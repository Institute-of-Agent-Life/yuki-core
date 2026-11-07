# 模型原生网页搜索

本轮 EXT-029 的核心交接使用 DeepSeek 自带的网页搜索。新部署在 `yuki.toml` 选择以下策略，模型配置文件的路径沿用该部署的 `model_profiles_file`。

```toml
[search]
mode = "native"
strategy = "native"
```

可直接使用候选包中的 `config/model_profiles.deepseek-native.example.toml`。如果已有模型配置，将现有 `routes.chat_agent` 所指 Profile 的连接字段设为以下值。保留该部署已有的预算、授权与其他路由，凭据由环境变量提供。

```toml
provider = "deepseek"
protocol = "responses"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-pro"
api_key_env = "DEEPSEEK_API_KEY"
search_mode = "native"
capabilities = ["tools", "native_web_search"]
```

Main 直接向所选模型的 `/responses` 发送带 `{"type":"web_search"}` 的请求。模型按需在服务端搜索或打开网页，原始响应里的 `web_search_call` 保留在该次响应上。供应商报告 `usage.server_tool_use.tool_search` 和 `tool_fetch` 时，原物理请求分别记下查询次数和读取次数；缺失值留空。一条请求可以对应多次查询。来源按服务端返回的事件和引用核对。DeepSeek 的 Responses 搜索支持机型为 `deepseek-v4-pro`。[官方 Responses 指南](https://api-docs.deepseek.com/guides/responses_api/)

此配置不安装 Tavily 插件，也不配置独立搜索连接。模型拒绝请求或没有搜索证据时，保留原结果。开发候选内的 `evidence/native-only-search.json` 记录原生工具声明、逐次记账、服务商拒绝以及重启后的回读。验证使用本机回环服务。
