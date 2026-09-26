use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub executable: String,
    #[serde(default)] pub accepts: Vec<String>,
    #[serde(default)] pub targets: Vec<String>,
    #[serde(default)] pub actions: Vec<PluginAction>,
    #[serde(default)] pub permissions: PluginPermissions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAction { pub id: String, pub label: String, #[serde(default)] pub produces: Vec<String>, #[serde(default)] pub cost: u8 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginPermissions { pub network: bool, pub filesystem_write: bool, pub dbus: bool }
