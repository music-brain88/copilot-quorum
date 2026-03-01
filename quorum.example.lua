-- Copilot Quorum Configuration Example
-- Copy this file to: ~/.config/copilot-quorum/init.lua
-- Plugins go in: ~/.config/copilot-quorum/plugins/*.lua (loaded alphabetically)
--
-- Boot sequence: Rust defaults -> Lua (init.lua + plugins) -> CLI arg overrides

-- ==================== Model Selection ====================
-- Role-based model configuration for different phases of execution.

-- Agent roles (used in Solo/Agent mode)
-- quorum.config.set("models.exploration", "gpt-5.2-codex")       -- Context gathering + low-risk tools
-- quorum.config.set("models.decision", "claude-sonnet-4.5")      -- Planning + high-risk tools
-- quorum.config.set("models.review", { "claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview" })

-- Interaction roles (used in Ensemble/Quorum mode)
-- quorum.config.set("models.participants", { "claude-opus-4.5", "gpt-5.2-codex", "gemini-3-pro-preview" })
-- quorum.config.set("models.moderator", "claude-opus-4.5")       -- Quorum Synthesis
-- quorum.config.set("models.ask", "claude-sonnet-4.5")           -- Ask (Q&A) interaction

-- ==================== Agent Behavior ====================
-- Controls autonomous agent execution behavior.

-- Consensus level: "solo" or "ensemble" (default: solo)
quorum.config.set("agent.consensus_level", "solo")
-- Phase scope: "full", "fast", or "plan-only" (default: full)
quorum.config.set("agent.phase_scope", "full")
-- Orchestration strategy: "quorum" or "debate" (default: quorum)
quorum.config.set("agent.strategy", "quorum")
-- Human-in-the-loop mode (default: interactive)
--   - interactive: Prompt user for decisions
--   - auto_reject: Automatically abort if revision limit exceeded
--   - auto_approve: Automatically approve last plan (use with caution!)
quorum.config.set("agent.hil_mode", "interactive")
-- Maximum plan revisions before human intervention (default: 3)
-- quorum.config.set("agent.max_plan_revisions", 3)

-- ==================== Output ====================

-- Output format: "full", "synthesis", or "json" (default: synthesis)
quorum.config.set("output.format", "synthesis")
-- Enable colored terminal output (default: true)
quorum.config.set("output.color", true)

-- ==================== Execution ====================
-- Controls agent loop limits.

-- quorum.config.set("execution.max_iterations", 20)     -- Max planning iterations (default: 20)
-- quorum.config.set("execution.max_tool_turns", 10)     -- Max tool turns per task (default: 10)

-- ==================== REPL ====================

-- Show progress indicators during processing (default: true)
quorum.config.set("repl.show_progress", true)
-- Path to history file (optional)
-- Default: ~/.local/share/copilot-quorum/history.txt
-- quorum.config.set("repl.history_file", "~/.local/share/copilot-quorum/history.txt")

-- ==================== TUI Input ====================
-- Terminal user interface settings for the modal input system.

-- Key to submit input (default: "enter")
quorum.config.set("tui.input.submit_key", "enter")
-- Key to insert a newline in multiline mode (default: "shift+enter")
quorum.config.set("tui.input.newline_key", "shift+enter")
-- Key to launch $EDITOR from Normal mode (default: "I")
quorum.config.set("tui.input.editor_key", "I")
-- What happens after editor saves: "return_to_insert" or "submit" (default: "return_to_insert")
quorum.config.set("tui.input.editor_action", "return_to_insert")
-- Maximum height for the input area in lines (default: 10)
quorum.config.set("tui.input.max_height", 10)
-- Whether input area grows dynamically with content (default: true)
quorum.config.set("tui.input.dynamic_height", true)
-- Whether to show context header in $EDITOR temp file (default: true)
quorum.config.set("tui.input.context_header", true)

-- ==================== TUI Layout ====================
-- Customize the TUI layout with presets and fine-grained surface/route control.

-- Layout preset: "default", "minimal", "wide", "stacked" (default: "default")
--   - default:  70/30 horizontal split (conversation + sidebar)
--   - minimal:  Full-width conversation, no sidebar
--   - wide:     60/20/20 three-pane horizontal split (conversation + progress + tools)
--   - stacked:  70/30 vertical split (conversation top, progress bottom)
quorum.config.set("tui.layout.preset", "default")
-- Terminal width threshold for responsive fallback to Minimal (default: 120)
-- Set to 0 to disable responsive fallback.
-- quorum.config.set("tui.layout.flex_threshold", 120)

-- ==================== Context Budget ====================
-- Controls how much task result context is retained between executions.
-- Prevents prompt bloat by truncating/summarizing older results.

-- quorum.config.set("context_budget.max_entry_bytes", 20000)    -- Max bytes per single task result
-- quorum.config.set("context_budget.max_total_bytes", 60000)    -- Max total bytes for all previous results
-- quorum.config.set("context_budget.recent_full_count", 3)      -- Recent results kept in full

-- ==================== Providers ====================
-- Provider-specific configuration for direct API access.
-- By default, all models are routed through the Copilot CLI backend.

quorum.providers.set_default("copilot")

-- AWS Bedrock provider (requires `bedrock` feature: cargo build --features bedrock)
-- Uses IAM authentication — no API key needed, just configured AWS credentials.
-- quorum.providers.bedrock({ region = "us-east-1", profile = "default", max_tokens = 8192, cross_region = false })

-- Explicit model -> provider routing overrides.
-- Maps model names to provider backends ("copilot", "anthropic", "openai", "bedrock", "azure").
-- quorum.providers.route("claude-sonnet-4.5", "bedrock")

-- Direct API providers (requires API keys)
-- quorum.providers.anthropic({ api_key = os.getenv("ANTHROPIC_API_KEY") })
-- quorum.providers.openai({ api_key = os.getenv("OPENAI_API_KEY") })

-- ==================== Custom Tools ====================
-- Define external CLI commands as first-class tools.
-- Parameter values are shell-escaped to prevent command injection.
-- Default risk_level is "high" (safe by default).

-- quorum.tools.register("my_tool", {
--     description = "My custom tool",
--     command = "echo {input}",
--     risk_level = "high",
--     parameters = {
--         input = { type = "string", description = "Input text", required = true },
--     }
-- })

-- ==================== Custom Keybindings ====================
-- quorum.keymap.set("normal", "q", "quit")
-- quorum.keymap.set("insert", "ctrl+s", "submit")
-- quorum.keymap.set("normal", "?", function() print("Help!") end)

-- ==================== Custom Commands ====================
-- quorum.command.register("hello", {
--     description = "Say hello",
--     fn = function(args) print("Hello, " .. (args or "world") .. "!") end,
-- })

-- ==================== Event Hooks ====================
-- quorum.on("ToolCallBefore", function(event)
--     -- Return false to cancel tool execution
--     return true
-- end)
-- quorum.on("ToolCallAfter", function(event)
--     -- event.tool_name, event.success, event.duration_ms
-- end)
-- quorum.on("PlanCreated", function(event)
--     -- event.objective, event.task_count
-- end)
