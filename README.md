# AgentKernel Capabilities

English version: [README.en.md](README.en.md)

友情链接：

- AgentKernel: https://github.com/cih1996/AgentKernel
- AgentKernel MCP Framework: https://github.com/cih1996/agentkernel-mcp-framework

---

AgentKernel Capabilities 是一个独立的 MCP stdio 能力服务，用 Rust 实现，向 MCP Client 暴露本地代码/系统工具：

- `glob`
- `grep`
- `read`
- `edit`
- `write`
- `bash`

适合接入 AgentKernel 或其他 MCP Client，为代码理解、文件修改和命令执行场景提供基础工具。

## 适用场景

- 给 AgentKernel 或其他 AI Runtime 提供本地代码工具。
- 作为 MCP Server 被第三方客户端加载。
- 配合 [AgentKernel MCP Framework](https://github.com/cih1996/agentkernel-mcp-framework) 使用，由 Framework 统一发现、注册和代理调用。

## 架构关系

```text
AgentKernel / Business App
  -> MCP Client / MCP Framework
  -> agentkernel-capabilities
  -> glob / grep / read / edit / write / bash
```

## MCP 配置

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

## 编译

```bash
cargo build --release
```

产物：

```bash
target/release/agentkernel-capabilities
```

## 通信方式

当前兼容用户已验证可用的 MCP Client / TypeScript SDK stdio 实现：

```text
一行一个 JSON-RPC 消息，以 \n 结尾
```

示例：

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"client","version":"0.1"}}}
```

MCP Client 通过 stdin 写入 JSON-RPC 行，并从 stdout 读取响应。

## MCP 方法

- `initialize`
- `notifications/initialized`
- `tools/list`
- `tools/call`
- `resources/list`
- `prompts/list`
- `logging/setLevel`
- `ping`

## 工具示例

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

## 与 AgentKernel MCP Framework 配合

如果业务端不想自己实现 MCP stdio 管理，可以使用：

- https://github.com/cih1996/agentkernel-mcp-framework

Framework 会负责：

1. 加载 MCP 配置。
2. 启动本服务。
3. 获取 `tools/list`。
4. 转换成可注册给 AgentKernel 的工具定义。
5. 代理执行 `tools/call`。
