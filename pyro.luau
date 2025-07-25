return {
	build_config = {
		max_jobs = 4,
		sandbox = true,
	},
	store_config = {
		store_path = "/pyro/store",
		db_path = "/pyro/database.sqlite",
		gc = {
			enabled = true,
			remove_older_than = "7d", -- Remove store items older than 7 days
		},
	},
}

-- # Build configuration
-- [build_config]
-- max_jobs = 4
-- sandbox = true
-- cache_dir = "C:\\pyro\\cache"
-- system_packages = false

-- # Store configuration
-- [store_config]
-- store_path = "/nix/store"  # On Windows: "C:\\pyro\\store"
-- gc_roots = ["/nix/var/nix/gcroots"]
-- auto_gc = false
-- max_store_size = 10737418240  # 10GB in bytes
