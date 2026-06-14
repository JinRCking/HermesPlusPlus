# HermesPlusPlus

HermesPlusPlus 是一个 Hermes CLI 的增强工具，让你能够自由切换不同的 AI 模型供应商。

## 功能特性

- 供应商配置管理：支持多个 API 供应商的配置
- 模型自由切换：自动从上游拉取模型列表，无需手动维护
- 协议自动转换：支持 Chat Completions 和 Responses API 之间的转换
- 本地代理服务：在本地端口运行代理，Hermes 通过代理访问上游 API
- 管理界面：直观的 Tauri GUI 管理工具

## 安装

```bash
# 克隆仓库
git clone https://github.com/BigPizzaV3/HermesPlusPlus.git
cd HermesPlusPlus

# 构建核心库
cd crates/hermes-plus-core
cargo build

# 构建管理器（需要 Tauri 环境）
cd ../../apps/hermes-plus-manager/src-tauri
cargo tauri build
```

## 使用

1. 打开 HermesPlusPlus 管理器
2. 添加供应商配置（Base URL + API Key）
3. 选择要使用的供应商
4. 启动代理服务
5. 在 Hermes 中将 API Base URL 设置为 `http://127.0.0.1:57421/v1`
6. 在 Hermes 的模型下拉菜单中即可看到该供应商的所有可用模型

## 配置项

- **Base URL**: 上游 API 的地址，如 `https://api.openai.com/v1`
- **API Key**: 上游 API 的访问密钥
- **协议类型**: Chat Completions 或 Responses API
- **模型列表**: 手动指定或从上游自动拉取

## 目录结构

```
HermesPlusPlus/
├── Cargo.toml                    # 工作区配置
├── crates/
│   └── hermes-plus-core/         # 核心库
│       ├── src/
│       │   ├── lib.rs            # 库入口
│       │   ├── settings.rs       # 设置管理
│       │   ├── relay_config.rs   # 供应商配置
│       │   ├── model_catalog.rs  # 模型目录
│       │   ├── proxy_server.rs   # 代理服务器
│       │   └── upstream_worktree.rs # 工作目录管理
│       └── Cargo.toml
└── apps/
    └── hermes-plus-manager/      # Tauri 管理应用
        ├── src-tauri/
        │   ├── src/
        │   │   ├── lib.rs        # Tauri 入口
        │   │   ├── main.rs       # 主函数
        │   │   └── commands.rs   # 前端命令
        │   ├── tauri.conf.json   # Tauri 配置
        │   └── Cargo.toml
        └── dist/
            └── index.html        # 管理界面
```

## 许可证

MIT
