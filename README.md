# AgentKernel Capabilities

[中文](#中文) | [English](#english)

Related projects / 友情链接：

- AgentKernel: https://github.com/cih1996/AgentKernel
- AgentKernel MCP Framework: https://github.com/cih1996/agentkernel-mcp-framework

---

## 中文

AgentKernel Capabilities 是一个独立的 MCP stdio 能力服务，用 Rust 实现，向 MCP Client 暴露本地代码/系统工具：

- `glob`
- `grep`
- `read`
- `edit`
- `write`
- `bash`

它的定位不是 AI Runtime，也不是 AgentKernel 内核的一部分，而是一个可插拔的能力套件。AgentKernel 本体只需要理解通用 Tool 抽象，具体能力可以通过 MCP 动态接入。

### 适用场景

- 给 AgentKernel 或其他 AI Runtime 提供本地代码工具。
- 作为 MCP Server 被第三方客户端加载。
- 配合 [AgentKernel MCP Framework](https://github.com/cih1996/agentkernel-mcp-framework) 使用，由 Framework 统一发现、注册和代理调用。

### 架构关系

```text
AgentKernel / Business App
  -> MCP Client / MCP Framework
  -> agentkernel-capabilities
  -> glob / grep / read / edit / write / bash
```

### MCP 配置

推荐使用 release 产物：

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

项目内也提供了示例：

- `mcp.json`
- `.mcp.json`

### 编译

```bash
cargo build --release
```

产物：

```bash
target/release/agentkernel-capabilities
```

### 通信方式

当前兼容用户已验证可用的 MCP Client / TypeScript SDK stdio 实现：

```text
一行一个 JSON-RPC 消息，以 \n 结尾
```

示例：

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"client","version":"0.1"}}}
```

注意：本服务不是 HTTP/TCP 服务。直接在终端运行不会主动输出内容，它会等待 MCP Client 从 stdin 写入 JSON-RPC 行。

### MCP 方法

- `initialize`
- `notifications/initialized`
- `tools/list`
- `tools/call`
- `resources/list`
- `prompts/list`
- `logging/setLevel`
- `ping`

### 工具示例

#### glob

```json
{"pattern":"crates/**/*.rs","path":"/absolute/path/to/workspace"}
```

#### grep

```json
{"pattern":"ToolManager","path":"/absolute/path/to/workspace","glob":"*.rs","outputMode":"content","context":2,"lineNumber":true,"ignoreCase":false,"headLimit":50,"offset":0,"multiline":false}
```

#### read

```json
{"file_path":"/absolute/path/to/file.rs","offset":0,"limit":120}
```

#### edit

```json
{"file_path":"/tmp/demo.txt","old_string":"hello","new_string":"hello agentkernel","replace_all":false}
```

#### write

```json
{"file_path":"/tmp/demo.txt","content":"hello agentkernel\n"}
```

#### bash

```json
{"command":"cargo test","description":"Run Rust tests","timeout":120000,"run_in_background":false,"dangerouslyDisableSandbox":false}
```

### 与 AgentKernel MCP Framework 配合

如果业务端不想自己实现 MCP stdio 管理，可以使用：

- https://github.com/cih1996/agentkernel-mcp-framework

Framework 会负责：

1. 加载 MCP 配置。
2. 启动本服务。
3. 获取 `tools/list`。
4. 转换成可注册给 AgentKernel 的工具定义。
5. 代理执行 `tools/call`。

---

## English

AgentKernel Capabilities is an independent MCP stdio capability server written in Rust. It exposes local code/system tools to MCP clients:

- `glob`
- `grep`
- `read`
- `edit`
- `write`
- `bash`

It is not an AI runtime and not part of the AgentKernel core. AgentKernel only needs to understand generic Tool abstractions, while concrete capabilities can be plugged in dynamically through MCP.

### Use Cases

- Provide local code tools for AgentKernel or other AI runtimes.
- Run as an MCP server loaded by third-party clients.
- Work with [AgentKernel MCP Framework](https://github.com/cih1996/agentkernel-mcp-framework), which handles discovery, registration, and tool-call proxying.

### Architecture

```text
AgentKernel / Business App
  -> MCP Client / MCP Framework
  -> agentkernel-capabilities
  -> glob / grep / read / edit / write / bash
```

### MCP Configuration

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

### Build

```bash
cargo build --release
```

Binary:

```bash
target/release/agentkernel-capabilities
```

### Transport

This project currently follows the newline-delimited JSON-RPC stdio behavior used by the verified MCP client / TypeScript SDK implementation:

```text
One JSON-RPC message per line, ending with \n
```

Example:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"client","version":"0.1"}}}
```

This is not an HTTP/TCP server. Running it directly in a terminal will not produce output until a client writes JSON-RPC lines to stdin.

### MCP Methods

- `initialize`
- `notifications/initialized`
- `tools/list`
- `tools/call`
- `resources/list`
- `prompts/list`
- `logging/setLevel`
- `ping`

### Tool Examples

#### glob

```json
{"pattern":"crates/**/*.rs","path":"/absolute/path/to/workspace"}
```

#### grep

```json
{"pattern":"ToolManager","path":"/absolute/path/to/workspace","glob":"*.rs","outputMode":"content","context":2,"lineNumber":true,"ignoreCase":false,"headLimit":50,"offset":0,"multiline":false}
```

#### read

```json
{"file_path":"/absolute/path/to/file.rs","offset":0,"limit":120}
```

#### edit

```json
{"file_path":"/tmp/demo.txt","old_string":"hello","new_string":"hello agentkernel","replace_all":false}
```

#### write

```json
{"file_path":"/tmp/demo.txt","content":"hello agentkernel\n"}
```

#### bash

```json
{"command":"cargo test","description":"Run Rust tests","timeout":120000,"run_in_background":false,"dangerouslyDisableSandbox":false}
```

### Working with AgentKernel MCP Framework

If your business layer does not want to manage MCP stdio directly, use:

- https://github.com/cih1996/agentkernel-mcp-framework

The framework handles:

1. Loading MCP configs.
2. Starting this server.
3. Fetching `tools/list`.
4. Converting tools into AgentKernel-registerable definitions.
5. Proxying `tools/call`.
