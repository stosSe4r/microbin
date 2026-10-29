# 随手分享 · MicroBin

基于 MicroBin 的中文轻量分享站：文字、图片、文件和短链接。

## 分享流程

上传完成后进入 `/upload/{id}`，结果页直接展示可复制的短链接与二维码。

| 地址 | 用途 |
| --- | --- |
| `/{id}` | 分享地址：纯单文件直达文件；纯网址跳转；文字、混合内容、多文件和加密内容进入预览 |
| `/upload/{id}` | 管理与预览页面，可复制管理地址、文件直链、图片 Markdown，编辑或删除 |
| `/p/{id}` | 预览页面 |
| `/file/{id}` | 第一个文件的下载地址；`?preview=true` 用于图片、视频在线展示 |
| `/raw/{id}` | 原始文本 |
| `/qr/{id}` | 单独展示同一个分享短链接的二维码 |

图片和视频的短地址跳转到带 `preview=true` 的文件地址，可以作为图床使用。多文件不会只分享第一个文件，带文字的文件不会遗漏说明。

「管理页面」是操作入口，和预览页沿用相同的访问规则，并非独立的私密权限。公开、仅链接可见的可编辑内容，知道链接的人仍然可以操作。二维码需设置 `MICROBIN_QR=true`，并配置完整的公开域名或短域名。

## 大文件上传

浏览器选择的文件合计超过 64 MiB 时，自动按文件顺序以 16 MiB 分片上传，最后沿用原有表单创建同一个分享。小文件仍直接上传。EdgeOne 的 800 MB 限制针对单个请求，因此多个文件合计超限也需要分片；分片不会提高站点配置的单文件上限（默认 2 GiB）。

无需新依赖或额外服务。上传页面显示进度、速度、中文错误和取消按钮，网络错误与 5xx 对当前分片最多重试两次，已确认的分片不重传。最后创建分享的请求不自动重试，避免重复创建；响应丢失时请先检查全部分享。

分片临时文件位于数据目录的 `.uploads/`，完成后直接移动到附件目录，不在内存中拼接整个文件。取消时删除，闲置 1 小时后定期清理；服务启动时清空未完成的临时文件。不支持刷新页面、服务重启后的断点续传。服务器加密仍遵守原来的加密大小限制，并沿用原有加密实现。

## Docker 部署

```sh
cp microbin.env.example microbin.env
# 修改 microbin.env 内的域名、站点名称等配置
sudo ./deploy.sh
```

也提供 `compose.yaml`，使用相同的环境文件、端口和数据目录。首次启动需创建 `data/` 并将其属主设为 `65532:65532`，然后执行 `docker compose up -d --build`。

服务监听 HTTP `31245`，CDN 将 `https://b64.top` 回源到该端口。配置中的 `MICROBIN_PUBLIC_PATH` 应为访客访问的 HTTPS 域名；独立短域名可配置在 `MICROBIN_SHORT_PATH`，同样指向此服务。站点名称来自 `MICROBIN_TITLE`。

同一端口支持 HTTP/1.1 和明文 HTTP/2（H2C prior knowledge），无需 SSL。可用 `curl --http2-prior-knowledge -sS -D - -o /dev/null http://源站:31245/` 验证；`curl --http2` 使用的 HTTP/1.1 Upgrade 方式不在支持范围内。EdgeOne 使用 HTTP 回源时可开启 HTTP/2 回源。

`deploy.sh` 从当前源码构建 `microbin-local:latest`，然后替换同名容器。数据保存在当前目录的 `data/`，使用 JSON 存储。默认永久保存、无需登录、公开分享，单文件上限 2 GiB。容器随 Docker 自动启动。

中文主题和交互已内置在 `templates/assets/theme.css`、`theme.js`，无需额外 Web 服务。主要上传、分享、列表和说明界面使用中文，部分原项目的管理与加密提示仍为英文；界面汉化脚本不会修改分享内容或文件名。需要自定义样式时仍可使用 `MICROBIN_CUSTOM_CSS`。

从之前使用自定义页脚脚本的部署切换时，删除环境变量中的 `MICROBIN_CUSTOM_CSS`、`MICROBIN_FOOTER_TEXT`，改用内置资源。

## 验证

```sh
cargo build --locked
cargo test --locked
python3 tests/share_flow.py
```

集成检查会在临时目录启动本地实例，覆盖文字、图片、普通文件、混合内容、多文件、网址、独立短域名、登录路由、原始内容和文件 Range 请求。

检查也会通过支持 HTTP/2 的 `curl` 验证同端口 HTTP/1.1 与 H2C 响应，检查分片重复提交、顺序和大小校验、取消与临时文件清理。加 `--large-upload` 可实际上传 801 MiB 并校验下载文件的 SHA-256（需约 1 GiB 临时磁盘空间）。

可选浏览器检查会验证实际上传、复制内容、二维码解码及手机布局：

```sh
npm install --no-save --package-lock=false playwright jsqr pngjs
npx playwright install chromium
python3 tests/share_flow.py --browser
```

可用 `CHROMIUM_PATH` 指定已有 Chromium，`SCREENSHOT_DIR` 保存预览截图，`MICROBIN_BINARY` 指定待验证的可执行文件。
