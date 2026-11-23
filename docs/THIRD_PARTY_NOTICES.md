# 第三方组件与许可

Browsory 自有代码使用 Apache License 2.0，完整文本见仓库根目录的 [LICENSE](../LICENSE)。

## 随应用分发的原生组件

| 组件 | 用途 | 许可与声明 |
|---|---|---|
| OpenSSL 3.6.3 | 为 Windows 构建中的 SQLCipher 提供静态加密库 | Apache-2.0；随附文本见 [`src-tauri/openssl/LICENSE.txt`](../src-tauri/openssl/LICENSE.txt)，版权归 OpenSSL Project Authors。 |
| SQLCipher 4.20.0 / SQLite 3.54.1 | 加密数据库和本地查询 | 使用 `sqlcipher-src` 与 `rusqlite` 的 bundled-sqlcipher 构建；上游许可文件见 [`docs/licenses/sqlcipher-src/`](licenses/sqlcipher-src/)。 |
| LibTomCrypt 1.18.2 | SQLCipher 加密实现的组成部分 | 上游提供 Public Domain 与 WTFPL 双重许可，文本见 [`LICENSE-libtomcrypt`](licenses/sqlcipher-src/LICENSE-libtomcrypt)。 |

`sqlcipher-src` 的包许可标识为 MIT、BSD-3-Clause、SQLite blessing 和 WTFPL。相关 MIT、SQLCipher 和 LibTomCrypt 许可文本已一并保存在 `docs/licenses/sqlcipher-src/`。OpenSSL 3.0 及后续版本使用 Apache License 2.0，详见[上游许可说明](https://openssl-library.org/source/license/index.html)。

## 其他依赖

前端和 Rust 依赖分别由 [`pnpm-lock.yaml`](../pnpm-lock.yaml) 与 [`src-tauri/Cargo.lock`](../src-tauri/Cargo.lock) 锁定。Tauri 及其插件、React、Vite、rusqlite、reqwest、rustls、serde、Argon2 和 Rust 加密库的许可信息以各自发行包内的声明为准；其常见许可为 MIT、Apache-2.0、BSD 或 ISC。构建前端依赖可使用 `pnpm audit --audit-level high` 检查已知安全公告。

`webpki-roots` 1.0.12 随 rustls 使用，包含根证书数据，采用 CDLA-Permissive-2.0；许可文本见 [`docs/licenses/webpki-roots/LICENSE`](licenses/webpki-roots/LICENSE)。

修改或重新分发依赖时，请同时保留相应上游许可证和版权声明。
