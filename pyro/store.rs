use crate::config::{PackageSpec, StoreConfig};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

/// Nix-like immutable package store
#[derive(Debug)]
pub struct PyroStore {
    config: StoreConfig,
    /// Maps package hash to store path
    store_db: HashMap<String, StorePath>,
}

#[derive(Debug, Clone)]
pub struct StorePath {
    pub hash: String,
    pub name: String,
    pub path: PathBuf,
    pub dependencies: Vec<String>,
    pub size: u64,
    pub created_at: SystemTime,
    pub last_accessed: SystemTime,
}

#[derive(Debug)]
pub struct BuildResult {
    pub store_path: StorePath,
    pub build_log: String,
    pub success: bool,
}

impl PyroStore {
    pub fn new(config: StoreConfig) -> Result<Self, Box<dyn std::error::Error>> {
        fs::create_dir_all(&config.store_path)?;
        
        let mut store = PyroStore {
            config,
            store_db: HashMap::new(),
        };
        
        store.load_store_db()?;
        Ok(store)
    }

    /// Generate content-addressable hash for a package
    pub fn compute_package_hash(&self, spec: &PackageSpec) -> String {
        let mut hasher = Sha256::new();
        
        // Hash package specification for reproducibility
        hasher.update(spec.name.as_bytes());
        if let Some(version) = &spec.version {
            hasher.update(version.as_bytes());
        }
        
        // Hash source
        match &spec.source {
            crate::config::PackageSource::Crates { name, version } => {
                hasher.update(b"crates");
                hasher.update(name.as_bytes());
                hasher.update(version.as_bytes());
            }
            crate::config::PackageSource::Git { url, rev } => {
                hasher.update(b"git");
                hasher.update(url.as_bytes());
                if let Some(rev) = rev {
                    hasher.update(rev.as_bytes());
                }
            }
            crate::config::PackageSource::Path { path } => {
                hasher.update(b"path");
                hasher.update(path.to_string_lossy().as_bytes());
            }
            crate::config::PackageSource::Url { url, hash } => {
                hasher.update(b"url");
                hasher.update(url.as_bytes());
                hasher.update(hash.as_bytes());
            }
        }
        
        // Hash dependencies
        for dep in &spec.build_inputs {
            hasher.update(dep.as_bytes());
        }
        for dep in &spec.runtime_inputs {
            hasher.update(dep.as_bytes());
        }
        
        // Hash environment
        let mut env_keys: Vec<_> = spec.environment.keys().collect();
        env_keys.sort();
        for key in env_keys {
            hasher.update(key.as_bytes());
            hasher.update(spec.environment[key].as_bytes());
        }
        
        format!("{:x}", hasher.finalize())[..32].to_string()
    }

    /// Get store path for a package
    pub fn get_store_path(&self, spec: &PackageSpec) -> PathBuf {
        let hash = self.compute_package_hash(spec);
        let name = format!("{}-{}", hash, spec.name);
        self.config.store_path.join(name)
    }

    /// Check if package exists in store
    pub fn package_exists(&self, spec: &PackageSpec) -> bool {
        let hash = self.compute_package_hash(spec);
        self.store_db.contains_key(&hash)
    }

    /// Add package to store
    pub fn add_package(&mut self, spec: &PackageSpec, build_result: BuildResult) -> Result<(), Box<dyn std::error::Error>> {
        let hash = self.compute_package_hash(spec);
        
        if build_result.success {
            self.store_db.insert(hash, build_result.store_path);
            self.save_store_db()?;
        }
        
        Ok(())
    }

    /// Get package from store
    pub fn get_package(&mut self, spec: &PackageSpec) -> Option<&mut StorePath> {
        let hash = self.compute_package_hash(spec);
        if let Some(store_path) = self.store_db.get_mut(&hash) {
            store_path.last_accessed = SystemTime::now();
            Some(store_path)
        } else {
            None
        }
    }

    /// Garbage collect unused packages
    pub fn garbage_collect(&mut self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut removed = Vec::new();
        let now = SystemTime::now();
        
        // Find packages not accessed in 30 days
        let mut to_remove = Vec::new();
        for (hash, store_path) in &self.store_db {
            if let Ok(duration) = now.duration_since(store_path.last_accessed) {
                if duration.as_secs() > 30 * 24 * 60 * 60 { // 30 days
                    to_remove.push(hash.clone());
                }
            }
        }
        
        // Remove old packages
        for hash in to_remove {
            if let Some(store_path) = self.store_db.remove(&hash) {
                if store_path.path.exists() {
                    fs::remove_dir_all(&store_path.path)?;
                }
                removed.push(store_path.name);
            }
        }
        
        self.save_store_db()?;
        Ok(removed)
    }

    /// Load store database from disk
    fn load_store_db(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let db_path = self.config.store_path.join(".pyro-store.json");
        if db_path.exists() {
            let content = fs::read_to_string(db_path)?;
            self.store_db = serde_json::from_str(&content)?;
        }
        Ok(())
    }

    /// Save store database to disk
    fn save_store_db(&self) -> Result<(), Box<dyn std::error::Error>> {
        let db_path = self.config.store_path.join(".pyro-store.json");
        let content = serde_json::to_string_pretty(&self.store_db)?;
        fs::write(db_path, content)?;
        Ok(())
    }

    /// Get store statistics
    pub fn get_stats(&self) -> StoreStats {
        let total_packages = self.store_db.len();
        let total_size: u64 = self.store_db.values().map(|p| p.size).sum();
        
        StoreStats {
            total_packages,
            total_size,
            store_path: self.config.store_path.clone(),
        }
    }
}

#[derive(Debug)]
pub struct StoreStats {
    pub total_packages: usize,
    pub total_size: u64,
    pub store_path: PathBuf,
}

// Implement serialization for StorePath
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct StorePathSerde {
    hash: String,
    name: String,
    path: PathBuf,
    dependencies: Vec<String>,
    size: u64,
    created_at: u64,
    last_accessed: u64,
}

impl Serialize for StorePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let serde_path = StorePathSerde {
            hash: self.hash.clone(),
            name: self.name.clone(),
            path: self.path.clone(),
            dependencies: self.dependencies.clone(),
            size: self.size,
            created_at: self.created_at.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default().as_secs(),
            last_accessed: self.last_accessed.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default().as_secs(),
        };
        serde_path.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StorePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let serde_path = StorePathSerde::deserialize(deserializer)?;
        Ok(StorePath {
            hash: serde_path.hash,
            name: serde_path.name,
            path: serde_path.path,
            dependencies: serde_path.dependencies,
            size: serde_path.size,
            created_at: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(serde_path.created_at),
            last_accessed: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(serde_path.last_accessed),
        })
    }
}