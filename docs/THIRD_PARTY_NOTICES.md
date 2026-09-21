# 第三方许可说明

Browsory 本体使用 Apache-2.0。发行包还包含以下主要依赖，许可证以其上游仓库为准。

## 前端

| 组件 | 许可 |
|---|---|
| React / React DOM | MIT |
| Zustand | MIT |
| Tailwind CSS | MIT |
| Lucide React | ISC |
| @tanstack/react-virtual | MIT |
| Vite | MIT |
| @tauri-apps/api 及官方插件 | MIT 或 Apache-2.0 |

## 后端 / 运行时

| 组件 | 许可 |
|---|---|
| Tauri 2 | MIT 或 Apache-2.0 |
| rusqlite / SQLite | MIT / blessing |
| reqwest / rustls | MIT 或 Apache-2.0 |
| argon2 / aes-gcm / ed25519-dalek | MIT 或 Apache-2.0 |
| chrono / serde / uuid / tracing | MIT 或 Apache-2.0 |
| keyring | MIT 或 Apache-2.0 |
| notify | CC0-1.0 或 MIT/Apache-2.0 |
| zip / flate2 / csv / rust_xlsxwriter | MIT 或 Apache-2.0 |

完整依赖树见 `pnpm-lock.yaml` 与 `src-tauri/Cargo.lock`。
