use anyhow::{Context, Result};
use scratchpad_core::*;
use scratchpad_storage::Repository;
use std::{
    path::Path,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

pub struct ServiceGuard {
    _thread: thread::JoinHandle<()>,
}

pub fn start_embedded() -> Result<ServiceGuard> {
    let paths = Paths::discover();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<()>>(1);

    let handle = thread::Builder::new()
        .name("scratchpad-service".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = ready_tx.send(Err(err.into()));
                    return;
                }
            };

            runtime.block_on(async move {
                if let Err(err) = serve_with_ready(paths, Some(ready_tx)).await {
                    eprintln!("[scratchpad-service] fatal: {err:#}");
                }
            });
        })?;

    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(ServiceGuard { _thread: handle }),
        Ok(Err(err)) => Err(err),
        Err(err) => Err(anyhow::anyhow!("service startup timed out: {err}")),
    }
}

pub fn run_headless() -> Result<()> {
    let paths = Paths::discover();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve_with_ready(paths, None))
}

async fn serve_with_ready(
    paths: Paths,
    ready: Option<mpsc::SyncSender<Result<()>>>,
) -> Result<()> {
    std::fs::create_dir_all(&paths.runtime_dir)?;

    if paths.socket.exists() {
        // A stale Unix socket is common after an unclean shutdown. Only remove
        // it if nothing is listening.
        if std::os::unix::net::UnixStream::connect(&paths.socket).is_ok() {
            if let Some(ready) = ready {
                let _ = ready.send(Err(anyhow::anyhow!(
                    "Scratchpad service is already running at {}",
                    paths.socket.display()
                )));
            }
            anyhow::bail!("service already running");
        }
        std::fs::remove_file(&paths.socket)?;
    }

    let repo = Arc::new(Mutex::new(Repository::open(&paths.db())?));
    let listener = UnixListener::bind(&paths.socket)
        .with_context(|| format!("bind {}", paths.socket.display()))?;

    if let Some(ready) = ready {
        let _ = ready.send(Ok(()));
    }

    eprintln!(
        "[scratchpad-service] listening on {}",
        paths.socket.display()
    );

    loop {
        let (stream, _) = listener.accept().await?;
        let repo = repo.clone();
        tokio::spawn(async move {
            if let Err(err) = handle(stream, repo).await {
                eprintln!("[scratchpad-service] client error: {err:#}");
            }
        });
    }
}

async fn handle(
    stream: UnixStream,
    repo: Arc<Mutex<Repository>>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let envelope: RequestEnvelope = serde_json::from_str(&line)?;
        let response = dispatch(&repo, envelope.request);
        let out = ResponseEnvelope {
            id: envelope.id,
            response,
        };

        writer
            .write_all(serde_json::to_string(&out)?.as_bytes())
            .await?;
        writer.write_all(b"\n").await?;
    }

    Ok(())
}

fn dispatch(
    repo: &Arc<Mutex<Repository>>,
    request: Request,
) -> Response {
    let repo = repo.lock().unwrap();

    let result: anyhow::Result<Response> = (|| {
        Ok(match request {
            Request::Ping => Response::Pong,
            Request::ListPages => Response::Pages(repo.list_pages()?),
            Request::CreatePage { name } => {
                let page = repo.create_page(&name)?;
                Response::Created { id: page.id }
            }
            Request::GetPage { page_id } => {
                Response::Page(repo.page_snapshot(page_id)?)
            }
            Request::AddObject {
                object,
                page_id,
                placement,
            } => {
                let id = object.id;
                repo.insert_object(
                    &object,
                    page_id,
                    placement.as_ref(),
                )?;
                Response::Created { id }
            }
            Request::ListObjects { limit } => {
                Response::Objects(repo.list_objects(limit)?)
            }
            Request::GetObject { object_id } => {
                Response::Object(
                    repo.get_object(object_id)?
                        .context("object not found")?,
                )
            }
            Request::RemoveObject { object_id } => {
                repo.remove_object(object_id)?;
                Response::Removed
            }
        })
    })();

    result.unwrap_or_else(|err| Response::Error {
        message: format!("{err:#}"),
    })
}

pub fn socket_is_live(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}
