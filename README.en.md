# AgentKernel Capabilities

中文版本: [README.md](README.md)

Related projects:

- AgentKernel: https://github.com/cih1996/AgentKernel
- AgentKernel MCP Framework: https://github.com/cih1996/agentkernel-mcp-framework

---

AgentKernel Capabilities is an independent MCP stdio capability server written in Rust. It exposes local code/system tools to MCP clients:

- `glob`
- `grep`
- `read`
- `edit`
- `write`
- `bash`

It is not an AI runtime and not part of the AgentKernel core. AgentKernel only needs to understand generic Tool abstractions, while concrete capabilities can be plugged in dynamically through MCP.

## Use Cases

- Provide local code tools for AgentKernel or other AI runtimes.
- Run as an MCP server loaded by third-party clients.
- Work with [AgentKernel MCP Framework](https://github.com/cih1996/agentkernel-mcp-framework), which handles discovery, registration, and tool-call proxying.

## Architecture

```text
AgentKernel / Business App
  -> MCP Client / MCP Framework
  -> agentkernel-capabilities
  -> glob / grep / read / edit / write / bash
```

## MCP Configuration

Recommended release configuration:

```json
{
  "mcpServers": {
    "local-code-suite": {
      "command": "/absolute/path/to/agentkernel-capabilities/target/release/agentkernel-capabilities",
      "args": ["--workspace", "/absolute/path/to/your/workspace"],
      "env": {}
    }
  }
}
```

Example config files are included:

- `mcp.json`
- `.mcp.json`

## Build

```bash
cargo build --release
```

Binary:

```bash
target/release/agentkernel-capabilities
```

## Transport

This project currently follows the newline-delimited JSON-RPC stdio behavior used by the verified MCP client / TypeScript SDK implementation:

```text
One JSON-RPC message per line, ending with \n
```

Example:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"client","version":"0.1"}}}
```

This is not an HTTP/TCP server. Running it directly in a terminal will not produce output until a client writes JSON-RPC lines to stdin.

## MCP Methods

- `initialize`
- `notifications/initialized`
- `tools/list`
- `tools/call`
- `resources/list`
- `prompts/list`
- `logging/setLevel`
- `ping`

## Tool Examples

### glob

```json
{"pattern":"crates/**/*.rs","path":"/absolute/path/to/workspace"}
```

### grep

```json
{"pattern":"ToolManager","path":"/absolute/path/to/workspace","glob":"*.rs","outputMode":"content","context":2,"lineNumber":true,"ignoreCase":false,"headLimit":50,"offset":0,"multiline":false}
```

### read

```json
{"file_path":"/absolute/path/to/file.rs","offset":0,"limit":120}
```

### edit

```json
{"file_path":"/tmp/demo.txt","old_string":"hello","new_string":"hello agentkernel","replace_all":false}
```

### write

```json
{"file_path":"/tmp/demo.txt","content":"hello agentkernel\n"}
```

### bash

```json
{"command":"cargo test","description":"Run Rust tests","timeout":120000,"run_in_background":false,"dangerouslyDisableSandbox":false}
```

## Working with AgentKernel MCP Framework

If your business layer does not want to manage MCP stdio directly, use:

- https://github.com/cih1996/agentkernel-mcp-framework

The framework handles:

1. Loading MCP configs.
2. Starting this server.
3. Fetching `tools/list`.
4. Converting tools into AgentKernel-registerable definitions.
5. Proxying `tools/call`.
