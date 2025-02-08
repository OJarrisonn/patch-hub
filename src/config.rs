use actix::{Actor, Addr, Handler, Message, MessageResponse};
use derive_getters::Getters;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    fs::{self, File as F},
    io,
    path::Path,
};
use thiserror::Error;

use crate::app::{cover_renderer::CoverRenderer, patch_renderer::PatchRenderer};

// #[cfg(test)]
// mod tests;

#[derive(Debug, Clone, Serialize, Deserialize, MessageResponse)]
pub struct Config {
    page_size: usize,
    patchsets_cache_dir: String,
    bookmarked_patchsets_path: String,
    mailing_lists_path: String,
    reviewed_patchsets_path: String,
    /// Logs directory
    logs_path: String,
    git_send_email_options: String,
    /// Base directory for all patch-hub cache
    cache_dir: String,
    /// Base directory for all patch-hub cache
    data_dir: String,
    /// Renderer to use for patch previews
    patch_renderer: PatchRenderer,
    /// Renderer to use for patchset covers
    cover_renderer: CoverRenderer,
    /// Maximum age of a log file in days
    max_log_age: usize,
    /// Map of tracked kernel trees
    kernel_trees: HashMap<String, KernelTree>,
    /// Target kernel tree to run actions
    target_kernel_tree: Option<String>,
    /// Flags to be use with `git am` command when applying patches
    git_am_options: String,
    git_am_branch_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Getters, Eq, PartialEq)]
pub struct KernelTree {
    /// Path to kernel tree in the filesystem
    path: String,
    /// Target branch
    branch: String,
}

impl Default for Config {
    fn default() -> Self {
        let cache_dir = format!("{}/.cache/patch_hub", env::var("HOME").unwrap());
        let data_dir = format!("{}/.local/share/patch_hub", env::var("HOME").unwrap());

        Config {
            page_size: 30,
            patchsets_cache_dir: format!("{cache_dir}/patchsets"),
            bookmarked_patchsets_path: format!("{data_dir}/bookmarked_patchsets.json"),
            mailing_lists_path: format!("{data_dir}/mailing_lists.json"),
            reviewed_patchsets_path: format!("{data_dir}/reviewed_patchsets.json"),
            logs_path: format!("{data_dir}/logs"),
            git_send_email_options: "--dry-run --suppress-cc=all".to_string(),
            patch_renderer: Default::default(),
            cover_renderer: Default::default(),
            cache_dir,
            data_dir,
            max_log_age: 30,
            kernel_trees: HashMap::new(),
            target_kernel_tree: None,
            git_am_options: String::new(),
            git_am_branch_prefix: String::from("patchset-"),
        }
    }
}

impl Config {
    /// Loads the configuration for patch-hub from the config file.
    ///
    /// Returns `None` if the config file is not found or if it's not a valid JSON.
    fn load_file() -> Option<Config> {
        if let Ok(config_path) = env::var("PATCH_HUB_CONFIG_PATH") {
            if Path::new(&config_path).is_file() {
                let file_contents = fs::read_to_string(&config_path).unwrap_or(String::new());
                if let Ok(config) = serde_json::from_str(&file_contents) {
                    return Some(config);
                }
            }
        }

        let config_path = format!(
            "{}/.config/patch-hub/config.json",
            env::var("HOME").unwrap()
        );
        if Path::new(&config_path).is_file() {
            let file_contents = fs::read_to_string(&config_path).unwrap_or(String::new());
            if let Ok(config) = serde_json::from_str(&file_contents) {
                return Some(config);
            }
        }

        None
    }

    fn override_with_env_vars(&mut self) {
        if let Ok(page_size) = env::var("PATCH_HUB_PAGE_SIZE") {
            self.page_size = page_size.parse().unwrap();
        };

        if let Ok(cache_dir) = env::var("PATCH_HUB_CACHE_DIR") {
            self.cache_dir = cache_dir;
        };

        if let Ok(data_dir) = env::var("PATCH_HUB_DATA_DIR") {
            self.data_dir = data_dir;
        };

        if let Ok(git_send_email_options) = env::var("PATCH_HUB_GIT_SEND_EMAIL_OPTIONS") {
            self.git_send_email_options = git_send_email_options;
        };

        if let Ok(patch_renderer) = env::var("PATCH_HUB_PATCH_RENDERER") {
            self.patch_renderer = patch_renderer.into();
        };
    }

    pub fn build() -> Self {
        let mut config = Self::load_file().unwrap_or_else(|| {
            let config = Self::default();
            // TODO: Better handle this error
            let _ = config.save_patch_hub_config();
            config
        });

        config.override_with_env_vars();

        config
    }

    fn save_patch_hub_config(&self) -> io::Result<()> {
        let config_path = if let Ok(path) = env::var("PATCH_HUB_CONFIG_PATH") {
            path
        } else {
            format!(
                "{}/.config/patch-hub/config.json",
                env::var("HOME").unwrap()
            )
        };

        let config_path = Path::new(&config_path);
        // We need to assure that the parent dir of `config_path` exists
        if let Some(parent_dir) = Path::parent(config_path) {
            fs::create_dir_all(parent_dir)?;
        }

        let tmp_filename = format!("{}.tmp", config_path.display());
        {
            let tmp_file = F::create(&tmp_filename)?;
            serde_json::to_writer_pretty(tmp_file, self)?;
        }
        fs::rename(tmp_filename, config_path)?;
        Ok(())
    }

    /// Creates the needed directories if they don't exist.
    /// The directories are defined during the Config build.
    ///
    /// This function must be called as soon as the Config is built so no other function attempt to use an inexistent folder.
    fn create_dirs(&self) -> io::Result<()> {
        let paths = vec![
            &self.cache_dir,
            &self.data_dir,
            &self.patchsets_cache_dir,
            &self.logs_path,
        ];

        for path in paths {
            fs::create_dir_all(path)?;
        }

        Ok(())
    }
}

impl Actor for Config {
    type Context = actix::Context<Self>;
}

/// Lists all config options that store a [`usize`] value.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum USizeConfig {
    PageSize,
    MaxLogAge,
}

/// Message to receive a [`usize`] value from the config.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "usize")]
pub struct GetUSize(pub USizeConfig);

/// Message to set a [`usize`] value in the config.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct SetUSize(pub USizeConfig, pub usize);

impl Handler<GetUSize> for Config {
    type Result = usize;
    fn handle(&mut self, msg: GetUSize, _: &mut Self::Context) -> Self::Result {
        match msg.0 {
            USizeConfig::PageSize => self.page_size,
            USizeConfig::MaxLogAge => self.max_log_age,
        }
    }
}

impl Handler<SetUSize> for Config {
    type Result = ();
    fn handle(&mut self, msg: SetUSize, _: &mut Self::Context) {
        match msg.0 {
            USizeConfig::PageSize => self.page_size = msg.1,
            USizeConfig::MaxLogAge => self.max_log_age = msg.1,
        }
    }
}

/// Lists all config options that store a [`String`] value.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StringConfig {
    PatchsetsCacheDir,
    BookmarkedPatchsetsPath,
    MailingListsPath,
    ReviewedPatchsetsPath,
    LogsPath,
    GitSendEmailOptions,
    CacheDir,
    DataDir,
    GitAmOptions,
    GitAmBranchPrefix,
}

/// Message to receive a [`String`] value from the config.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "String")]
pub struct GetString(pub StringConfig);

/// Message to set a [`String`] value in the config.
#[derive(Debug, Clone, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct SetString(pub StringConfig, pub String);

impl Handler<GetString> for Config {
    type Result = String;
    fn handle(&mut self, msg: GetString, _: &mut Self::Context) -> Self::Result {
        match msg.0 {
            StringConfig::PatchsetsCacheDir => &self.patchsets_cache_dir,
            StringConfig::BookmarkedPatchsetsPath => &self.bookmarked_patchsets_path,
            StringConfig::MailingListsPath => &self.mailing_lists_path,
            StringConfig::ReviewedPatchsetsPath => &self.reviewed_patchsets_path,
            StringConfig::LogsPath => &self.logs_path,
            StringConfig::GitSendEmailOptions => &self.git_send_email_options,
            StringConfig::CacheDir => &self.cache_dir,
            StringConfig::DataDir => &self.data_dir,
            StringConfig::GitAmOptions => &self.git_am_options,
            StringConfig::GitAmBranchPrefix => &self.git_am_branch_prefix,
        }
        .clone()
    }
}

impl Handler<SetString> for Config {
    type Result = ();
    fn handle(&mut self, msg: SetString, _: &mut Self::Context) {
        match msg.0 {
            StringConfig::PatchsetsCacheDir => self.cache_dir = msg.1,
            StringConfig::BookmarkedPatchsetsPath => self.bookmarked_patchsets_path = msg.1,
            StringConfig::MailingListsPath => self.mailing_lists_path = msg.1,
            StringConfig::ReviewedPatchsetsPath => self.reviewed_patchsets_path = msg.1,
            StringConfig::LogsPath => self.logs_path = msg.1,
            StringConfig::GitSendEmailOptions => self.git_send_email_options = msg.1,
            StringConfig::CacheDir => self.cache_dir = msg.1,
            StringConfig::DataDir => self.data_dir = msg.1,
            StringConfig::GitAmOptions => self.git_am_options = msg.1,
            StringConfig::GitAmBranchPrefix => self.git_am_branch_prefix = msg.1,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "PatchRenderer")]
pub struct GetPatchRenderer;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct SetPatchRenderer(pub PatchRenderer);

impl Handler<GetPatchRenderer> for Config {
    type Result = PatchRenderer;
    fn handle(&mut self, _: GetPatchRenderer, _: &mut Self::Context) -> Self::Result {
        self.patch_renderer
    }
}

impl Handler<SetPatchRenderer> for Config {
    type Result = ();
    fn handle(&mut self, msg: SetPatchRenderer, _: &mut Self::Context) {
        self.patch_renderer = msg.0;
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "CoverRenderer")]
pub struct GetCoverRenderer;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "()")]
pub struct SetCoverRenderer(pub CoverRenderer);

impl Handler<GetCoverRenderer> for Config {
    type Result = CoverRenderer;

    fn handle(&mut self, _: GetCoverRenderer, _: &mut Self::Context) -> Self::Result {
        self.cover_renderer
    }
}

impl Handler<SetCoverRenderer> for Config {
    type Result = ();
    fn handle(&mut self, msg: SetCoverRenderer, _: &mut Self::Context) -> Self::Result {
        self.cover_renderer = msg.0;
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "Vec<String>")]
pub struct GetKernelTrees;

impl Handler<GetKernelTrees> for Config {
    type Result = Vec<String>;
    fn handle(&mut self, _: GetKernelTrees, _: &mut Self::Context) -> Self::Result {
        self.kernel_trees.keys().cloned().collect()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Message)]
#[rtype(result = "Result<KernelTree, String>")]
pub struct GetKernelTree(pub String);

impl Handler<GetKernelTree> for Config {
    type Result = Result<KernelTree, String>;
    fn handle(&mut self, msg: GetKernelTree, _: &mut Self::Context) -> Self::Result {
        self.kernel_trees.get(&msg.0).cloned().ok_or(msg.0)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "Option<String>")]
pub struct GetTargetKernelTree;

impl Handler<GetTargetKernelTree> for Config {
    type Result = Option<String>;
    fn handle(&mut self, _: GetTargetKernelTree, _: &mut Self::Context) -> Self::Result {
        self.target_kernel_tree.clone()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "Result<(), FileError>")]
pub enum File {
    Load,
    Save,
    MkDirs,
}

/// Erro type returned by file related operations on a config
#[derive(Debug, Error)]
pub enum FileError {
    #[error(transparent)]
    Io(io::Error),
    #[error("Config file not found")]
    NotFound,
}

impl Handler<File> for Config {
    type Result = Result<(), FileError>;
    fn handle(&mut self, msg: File, _: &mut Self::Context) -> Self::Result {
        match msg {
            File::Load => {
                if let Some(config) = Config::load_file() {
                    *self = config;
                    Ok(())
                } else {
                    Err(FileError::NotFound)
                }
            }
            File::Save => self.save_patch_hub_config().map_err(FileError::Io),
            File::MkDirs => self.create_dirs().map_err(FileError::Io),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Message)]
#[rtype(result = "Config")]
pub struct Clone;

impl Handler<Clone> for Config {
    type Result = Config;
    fn handle(&mut self, _: Clone, _: &mut Self::Context) -> Self::Result {
        self.clone()
    }
}

pub trait ConfigActor {
    /// Gets a config option which value is a usize
    async fn usize(&self, config: USizeConfig) -> usize;

    /// Sets a config option which value is a usize
    async fn set_usize(&self, config: USizeConfig, value: usize);

    /// Get a config option which value is a string
    async fn string(&self, config: StringConfig) -> String;

    /// Set a config option which value is a string
    async fn set_string(&self, config: StringConfig, value: String);

    /// Gets the patch renderer
    async fn patch_renderer(&self) -> PatchRenderer;

    /// Sets the patch renderer
    async fn set_patch_renderer(&self, renderer: PatchRenderer);

    /// Gets the cover renderer
    async fn cover_renderer(&self) -> CoverRenderer;

    /// Sets the cover renderer
    async fn set_cover_renderer(&self, renderer: CoverRenderer);

    /// Get the list of registered kernel trees
    #[allow(dead_code)]
    async fn kernel_trees(&self) -> Vec<String>;

    /// Get a kernel tree by it's name
    async fn kernel_tree(&self, tree: String) -> Result<KernelTree, String>;

    /// Get the name of the
    async fn target_kernel_tree(&self) -> Option<String>;

    /// Reloads the config from the config file
    #[allow(dead_code)]
    async fn reload(&self) -> Result<(), FileError>;

    /// Saves the config to the config file
    async fn save(&self) -> Result<(), FileError>;

    /// Creates all the directories needed by patch-hub
    async fn mkdirs(&self) -> Result<(), FileError>;

    async fn cloned(&self) -> Config;
}

impl ConfigActor for Addr<Config> {
    async fn usize(&self, config: USizeConfig) -> usize {
        self.send(GetUSize(config))
            .await
            .expect("Failed to get usize from config, Config actor is dead")
    }

    async fn set_usize(&self, config: USizeConfig, value: usize) {
        self.send(SetUSize(config, value))
            .await
            .expect("Failed to set usize in config, Config actor is dead")
    }

    async fn string(&self, config: StringConfig) -> String {
        self.send(GetString(config))
            .await
            .expect("Failed to get string from config, Config actor is dead")
    }

    async fn set_string(&self, config: StringConfig, value: String) {
        self.send(SetString(config, value))
            .await
            .expect("Failed to set string in config, Config actor is dead")
    }

    async fn patch_renderer(&self) -> PatchRenderer {
        self.send(GetPatchRenderer)
            .await
            .expect("Failed to get patch renderer, Config actor is dead")
    }

    async fn set_patch_renderer(&self, renderer: PatchRenderer) {
        self.send(SetPatchRenderer(renderer))
            .await
            .expect("Failed to set patch renderer, Config actor is dead")
    }

    async fn cover_renderer(&self) -> CoverRenderer {
        self.send(GetCoverRenderer)
            .await
            .expect("Failed to get cover renderer, Config actor is dead")
    }

    async fn set_cover_renderer(&self, renderer: CoverRenderer) {
        self.send(SetCoverRenderer(renderer))
            .await
            .expect("Failed to set cover renderer, Config actor is dead")
    }

    async fn kernel_trees(&self) -> Vec<String> {
        self.send(GetKernelTrees)
            .await
            .expect("Failed to get kernel trees, Config actor is dead")
    }

    async fn kernel_tree(&self, tree: String) -> Result<KernelTree, String> {
        self.send(GetKernelTree(tree))
            .await
            .expect("Failed to get a kernel tree, Config actor is dead")
    }

    async fn target_kernel_tree(&self) -> Option<String> {
        self.send(GetTargetKernelTree)
            .await
            .expect("Failed to get the target kernel tree, Config actor is dead")
    }

    async fn reload(&self) -> Result<(), FileError> {
        self.send(File::Load)
            .await
            .expect("Failed to load config, Config actor is dead")
    }

    async fn save(&self) -> Result<(), FileError> {
        self.send(File::Save)
            .await
            .expect("Failed to save config, Config actor is dead")
    }

    async fn mkdirs(&self) -> Result<(), FileError> {
        self.send(File::MkDirs)
            .await
            .expect("Failed to create dirs, Config actor is dead")
    }

    async fn cloned(&self) -> Config {
        self.send(Clone)
            .await
            .expect("Failed to clone the config, Config actor is dead")
    }
}
