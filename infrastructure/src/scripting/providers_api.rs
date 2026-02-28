//! `quorum.providers` Lua API — provider configuration at init time.
//!
//! Lets init.lua configure provider settings such as default provider,
//! model-to-provider routing, and provider-specific parameters:
//!
//! ```lua
//! quorum.providers.set_default("bedrock")
//! quorum.providers.route("claude-sonnet-4.6", "bedrock")
//! quorum.providers.bedrock({ region = "us-west-2", profile = "dev-ai" })
//! quorum.providers.anthropic({ api_key = os.getenv("ANTHROPIC_API_KEY") })
//! quorum.providers.openai({ api_key = os.getenv("OPENAI_API_KEY") })
//! ```

use mlua::prelude::*;
use std::sync::{Arc, Mutex};

use quorum_domain::ProviderConfig;

/// Register the `quorum.providers` table on the given `quorum` global.
///
/// Each method mutates the shared `ProviderConfig` so that the CLI
/// can read back the final state after init.lua and plugins have run.
pub fn register_providers_api(
    lua: &Lua,
    quorum_table: &LuaTable,
    config: Arc<Mutex<ProviderConfig>>,
) -> LuaResult<()> {
    let providers = lua.create_table()?;

    // quorum.providers.set_default(provider_name)
    {
        let cfg = Arc::clone(&config);
        let set_default_fn = lua.create_function(move |_, name: String| {
            let mut guard = cfg.lock().map_err(|e| {
                LuaError::external(format!("providers config lock poisoned: {}", e))
            })?;
            guard.default = Some(name);
            Ok(())
        })?;
        providers.set("set_default", set_default_fn)?;
    }

    // quorum.providers.route(model_name, provider_name)
    {
        let cfg = Arc::clone(&config);
        let route_fn = lua.create_function(move |_, (model, provider): (String, String)| {
            let mut guard = cfg.lock().map_err(|e| {
                LuaError::external(format!("providers config lock poisoned: {}", e))
            })?;
            guard.routing.insert(model, provider);
            Ok(())
        })?;
        providers.set("route", route_fn)?;
    }

    // quorum.providers.bedrock({ region, profile, max_tokens, cross_region })
    {
        let cfg = Arc::clone(&config);
        let bedrock_fn = lua.create_function(move |_, table: LuaTable| {
            let mut guard = cfg.lock().map_err(|e| {
                LuaError::external(format!("providers config lock poisoned: {}", e))
            })?;
            if let Ok(v) = table.get::<String>("region") {
                guard.bedrock.region = v;
            }
            if let Ok(v) = table.get::<Option<String>>("profile") {
                guard.bedrock.profile = v;
            }
            if let Ok(v) = table.get::<u32>("max_tokens") {
                guard.bedrock.max_tokens = v;
            }
            if let Ok(v) = table.get::<Option<bool>>("cross_region") {
                guard.bedrock.cross_region = v;
            }
            Ok(())
        })?;
        providers.set("bedrock", bedrock_fn)?;
    }

    // quorum.providers.anthropic({ api_key, api_key_env, base_url, max_tokens, api_version })
    {
        let cfg = Arc::clone(&config);
        let anthropic_fn = lua.create_function(move |_, table: LuaTable| {
            let mut guard = cfg.lock().map_err(|e| {
                LuaError::external(format!("providers config lock poisoned: {}", e))
            })?;
            if let Ok(v) = table.get::<String>("api_key") {
                guard.anthropic.api_key = Some(v);
            }
            if let Ok(v) = table.get::<String>("api_key_env") {
                guard.anthropic.api_key_env = v;
            }
            if let Ok(v) = table.get::<String>("base_url") {
                guard.anthropic.base_url = v;
            }
            if let Ok(v) = table.get::<u32>("max_tokens") {
                guard.anthropic.max_tokens = v;
            }
            if let Ok(v) = table.get::<String>("api_version") {
                guard.anthropic.api_version = v;
            }
            Ok(())
        })?;
        providers.set("anthropic", anthropic_fn)?;
    }

    // quorum.providers.openai({ api_key, api_key_env, base_url, max_tokens })
    {
        let cfg = Arc::clone(&config);
        let openai_fn = lua.create_function(move |_, table: LuaTable| {
            let mut guard = cfg.lock().map_err(|e| {
                LuaError::external(format!("providers config lock poisoned: {}", e))
            })?;
            if let Ok(v) = table.get::<String>("api_key") {
                guard.openai.api_key = Some(v);
            }
            if let Ok(v) = table.get::<String>("api_key_env") {
                guard.openai.api_key_env = v;
            }
            if let Ok(v) = table.get::<String>("base_url") {
                guard.openai.base_url = v;
            }
            if let Ok(v) = table.get::<u32>("max_tokens") {
                guard.openai.max_tokens = v;
            }
            Ok(())
        })?;
        providers.set("openai", openai_fn)?;
    }

    quorum_table.set("providers", providers)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_lua_with_providers() -> (Lua, Arc<Mutex<ProviderConfig>>) {
        let lua = Lua::new();
        let config = Arc::new(Mutex::new(ProviderConfig::default()));
        let quorum = lua.create_table().unwrap();
        register_providers_api(&lua, &quorum, Arc::clone(&config)).unwrap();
        lua.globals().set("quorum", quorum).unwrap();
        (lua, config)
    }

    #[test]
    fn test_set_default() {
        let (lua, config) = make_lua_with_providers();
        lua.load(r#"quorum.providers.set_default("bedrock")"#)
            .exec()
            .unwrap();
        let guard = config.lock().unwrap();
        assert_eq!(guard.default, Some("bedrock".to_string()));
    }

    #[test]
    fn test_route() {
        let (lua, config) = make_lua_with_providers();
        lua.load(r#"quorum.providers.route("claude-sonnet-4.6", "bedrock")"#)
            .exec()
            .unwrap();
        let guard = config.lock().unwrap();
        assert_eq!(
            guard.routing.get("claude-sonnet-4.6"),
            Some(&"bedrock".to_string())
        );
    }

    #[test]
    fn test_bedrock_config() {
        let (lua, config) = make_lua_with_providers();
        lua.load(r#"quorum.providers.bedrock({ region = "us-west-2", profile = "dev-ai", max_tokens = 4096, cross_region = true })"#)
            .exec()
            .unwrap();
        let guard = config.lock().unwrap();
        assert_eq!(guard.bedrock.region, "us-west-2");
        assert_eq!(guard.bedrock.profile, Some("dev-ai".to_string()));
        assert_eq!(guard.bedrock.max_tokens, 4096);
        assert_eq!(guard.bedrock.cross_region, Some(true));
    }

    #[test]
    fn test_bedrock_partial_config() {
        let (lua, config) = make_lua_with_providers();
        lua.load(r#"quorum.providers.bedrock({ region = "ap-northeast-1" })"#)
            .exec()
            .unwrap();
        let guard = config.lock().unwrap();
        assert_eq!(guard.bedrock.region, "ap-northeast-1");
        assert_eq!(guard.bedrock.max_tokens, 8192);
        assert!(guard.bedrock.profile.is_none());
    }

    #[test]
    fn test_anthropic_config() {
        let (lua, config) = make_lua_with_providers();
        lua.load(r#"quorum.providers.anthropic({ api_key = "sk-test-123", base_url = "https://custom.api.com", max_tokens = 16384 })"#)
            .exec()
            .unwrap();
        let guard = config.lock().unwrap();
        assert_eq!(guard.anthropic.api_key, Some("sk-test-123".to_string()));
        assert_eq!(guard.anthropic.base_url, "https://custom.api.com");
        assert_eq!(guard.anthropic.max_tokens, 16384);
        assert_eq!(guard.anthropic.api_key_env, "ANTHROPIC_API_KEY");
    }

    #[test]
    fn test_openai_config() {
        let (lua, config) = make_lua_with_providers();
        lua.load(r#"quorum.providers.openai({ api_key = "sk-openai-test", base_url = "https://azure.openai.com", max_tokens = 32000 })"#)
            .exec()
            .unwrap();
        let guard = config.lock().unwrap();
        assert_eq!(guard.openai.api_key, Some("sk-openai-test".to_string()));
        assert_eq!(guard.openai.base_url, "https://azure.openai.com");
        assert_eq!(guard.openai.max_tokens, 32000);
    }

    #[test]
    fn test_multiple_calls_compound() {
        let (lua, config) = make_lua_with_providers();
        lua.load(
            r#"
            quorum.providers.set_default("anthropic")
            quorum.providers.route("gpt-5.2-codex", "openai")
            quorum.providers.route("claude-opus-4.5", "bedrock")
            quorum.providers.anthropic({ api_key = "sk-ant" })
            quorum.providers.openai({ base_url = "https://my-openai.com" })
            quorum.providers.bedrock({ region = "eu-west-1" })
        "#,
        )
        .exec()
        .unwrap();

        let guard = config.lock().unwrap();
        assert_eq!(guard.default, Some("anthropic".to_string()));
        assert_eq!(guard.routing.len(), 2);
        assert_eq!(guard.anthropic.api_key, Some("sk-ant".to_string()));
        assert_eq!(guard.openai.base_url, "https://my-openai.com");
        assert_eq!(guard.bedrock.region, "eu-west-1");
    }
}
