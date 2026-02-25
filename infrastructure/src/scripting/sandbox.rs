//! Lua sandbox — blocks C module loading for ABI safety.
//!
//! Following the WezTerm pattern, we trust user Lua code (it's their init.lua)
//! but block C extension modules to prevent ABI incompatibility crashes.

use mlua::prelude::*;

/// Apply sandbox restrictions to the Lua VM.
///
/// Currently blocks:
/// - `package.loadlib` — prevents loading arbitrary .so/.dll
/// - `package.cpath` — clears the C module search path
///
/// User Lua code is trusted (it's their own init.lua), so standard
/// library functions like `io`, `os`, `string` remain available.
pub fn apply_sandbox(lua: &Lua) -> LuaResult<()> {
    lua.load(
        r#"
        -- Block C module loading (ABI safety)
        package.loadlib = nil
        package.cpath = ''
    "#,
    )
    .exec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_blocks_loadlib() {
        let lua = Lua::new();
        apply_sandbox(&lua).unwrap();

        let result: LuaValue = lua.globals().get::<LuaTable>("package").unwrap()
            .get("loadlib").unwrap();
        assert_eq!(result, LuaValue::Nil);
    }

    #[test]
    fn test_sandbox_clears_cpath() {
        let lua = Lua::new();
        apply_sandbox(&lua).unwrap();

        let cpath: String = lua.globals().get::<LuaTable>("package").unwrap()
            .get("cpath").unwrap();
        assert_eq!(cpath, "");
    }

    #[test]
    fn test_sandbox_preserves_standard_libs() {
        let lua = Lua::new();
        apply_sandbox(&lua).unwrap();

        // string.upper should still work
        let result: String = lua.load("string.upper('hello')").eval().unwrap();
        assert_eq!(result, "HELLO");

        // table.concat should still work
        let result: String = lua
            .load("table.concat({'a', 'b', 'c'}, ', ')")
            .eval()
            .unwrap();
        assert_eq!(result, "a, b, c");
    }

    #[test]
    fn test_sandbox_require_lua_modules_still_works() {
        let lua = Lua::new();
        apply_sandbox(&lua).unwrap();

        // Pure Lua require should still work conceptually
        // (will fail with "module not found" but won't crash from C loading)
        let result = lua.load("pcall(require, 'nonexistent')").eval::<(bool, String)>();
        assert!(result.is_ok());
        let (ok, _msg) = result.unwrap();
        assert!(!ok); // Fails to find module, but doesn't crash
    }
}
