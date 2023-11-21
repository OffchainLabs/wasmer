use std::sync::Arc;

use anyhow::Context;
use derivative::*;
use once_cell::sync::OnceCell;
use semver::Version;
use virtual_fs::FileSystem;
use webc::{compat::SharedBytes, Container};

use crate::{
    runtime::{
        module_cache::ModuleHash,
        resolver::{PackageId, PackageInfo, PackageSpecifier, ResolveError},
    },
    Runtime,
};

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackageCommand {
    name: String,
    metadata: webc::metadata::Command,
    #[derivative(Debug = "ignore")]
    pub(crate) atom: SharedBytes,
    hash: OnceCell<ModuleHash>,
}

impl BinaryPackageCommand {
    pub fn new(name: String, metadata: webc::metadata::Command, atom: SharedBytes) -> Self {
        Self {
            name,
            metadata,
            atom,
            hash: OnceCell::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn metadata(&self) -> &webc::metadata::Command {
        &self.metadata
    }

    /// Get a reference to this [`BinaryPackageCommand`]'s atom.
    ///
    /// The address of the returned slice is guaranteed to be stable and live as
    /// long as the [`BinaryPackageCommand`].
    pub fn atom(&self) -> &[u8] {
        &self.atom
    }

    pub fn hash(&self) -> &ModuleHash {
        self.hash.get_or_init(|| ModuleHash::sha256(self.atom()))
    }
}

/// A WebAssembly package that has been loaded into memory.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct BinaryPackage {
    pub package_name: String,
    pub when_cached: Option<u128>,
    /// The name of the [`BinaryPackageCommand`] which is this package's
    /// entrypoint.
    pub entrypoint_cmd: Option<String>,
    pub hash: OnceCell<ModuleHash>,
    pub webc_fs: Arc<dyn FileSystem + Send + Sync>,
    pub commands: Vec<BinaryPackageCommand>,
    pub uses: Vec<String>,
    pub version: Version,
    pub module_memory_footprint: u64,
    pub file_system_memory_footprint: u64,
}

impl BinaryPackage {
    /// Load a [`webc::Container`] and all its dependencies into a
    /// [`BinaryPackage`].
    pub async fn from_webc(
        container: &Container,
        rt: &(dyn Runtime + Send + Sync),
    ) -> Result<Self, anyhow::Error> {
        let source = rt.source();
        let root = PackageInfo::from_manifest(container.manifest())?;
        let root_id = PackageId {
            package_name: root.name.clone(),
            version: root.version.clone(),
        };

        let resolution = crate::runtime::resolver::resolve(&root_id, &root, &*source).await?;
        let pkg = rt
            .package_loader()
            .load_package_tree(container, &resolution)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(pkg)
    }

    /// Load a [`BinaryPackage`] and all its dependencies from a registry.
    pub async fn from_registry(
        specifier: &PackageSpecifier,
        runtime: &(dyn Runtime + Send + Sync),
    ) -> Result<Self, anyhow::Error> {
        let source = runtime.source();
        let root_summary =
            source
                .latest(specifier)
                .await
                .map_err(|error| ResolveError::Registry {
                    package: specifier.clone(),
                    error,
                })?;
        let root = runtime.package_loader().load(&root_summary).await?;
        let id = root_summary.package_id();

        let resolution = crate::runtime::resolver::resolve(&id, &root_summary.pkg, &source)
            .await
            .context("Dependency resolution failed")?;
        let pkg = runtime
            .package_loader()
            .load_package_tree(&root, &resolution)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(pkg)
    }

    pub fn get_command(&self, name: &str) -> Option<&BinaryPackageCommand> {
        self.commands.iter().find(|cmd| cmd.name() == name)
    }

    /// Get the bytes for the entrypoint command.
    pub fn entrypoint_bytes(&self) -> Option<&[u8]> {
        self.entrypoint_cmd
            .as_deref()
            .and_then(|name| self.get_command(name))
            .map(|entry| entry.atom())
    }

    pub fn hash(&self) -> ModuleHash {
        *self.hash.get_or_init(|| {
            if let Some(entry) = self.entrypoint_bytes() {
                ModuleHash::sha256(entry)
            } else {
                ModuleHash::sha256(self.package_name.as_bytes())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use virtual_fs::AsyncReadExt;

    use crate::{runtime::task_manager::VirtualTaskManager, PluggableRuntime};

    use super::*;

    fn task_manager() -> Arc<dyn VirtualTaskManager + Send + Sync> {
        cfg_if::cfg_if! {
            if #[cfg(feature = "sys-threads")] {
                Arc::new(crate::runtime::task_manager::tokio::TokioTaskManager::new(tokio::runtime::Handle::current()))
            } else {
                unimplemented!("Unable to get the task manager")
            }
        }
    }

    #[tokio::test]
    #[cfg_attr(
        not(feature = "sys-threads"),
        ignore = "The tokio task manager isn't available on this platform"
    )]
    async fn fs_table_can_map_directories_to_different_names() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "a dummy package"

            [fs]
            "/public" = "./out"
        "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();
        let out = temp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let file_txt = "Hello, World!";
        std::fs::write(out.join("file.txt"), file_txt).unwrap();
        let webc: Container = webc::wasmer_package::Package::from_manifest(manifest)
            .unwrap()
            .into();
        let tasks = task_manager();
        let runtime = PluggableRuntime::new(tasks);

        let pkg = BinaryPackage::from_webc(&webc, &runtime).await.unwrap();

        // We should have mapped "./out/file.txt" on the host to
        // "/public/file.txt" on the guest.
        let mut f = pkg
            .webc_fs
            .new_open_options()
            .read(true)
            .open("/public/file.txt")
            .unwrap();
        let mut buffer = String::new();
        f.read_to_string(&mut buffer).await.unwrap();
        assert_eq!(buffer, file_txt);
    }
}
