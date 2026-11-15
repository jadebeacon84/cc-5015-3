# 建筑模型剖切工具（archisection）

浏览器内移动剖切平面的模型操作视窗；Rust 服务计算网格与平面的**真实交线**，生成带来源归属的二维轮廓与封口模型，并可冻结、导出。

不是渲染裁切：界面里看到的每一条轮廓、每一个封口三角面，都来自内核返回的几何数据（`/api/section`），可在“选中详情”里查到交点坐标与来源面。

---

## 1. 界面怎样启动

### 方式 A：一条命令（推荐，最终交付形态）

```bash
./start.sh                      # 首次会自动编译内核与前端，随后在 8787 端口启动
# 或指定端口：PORT=8791 ./start.sh
```

浏览器打开脚本打印的 **http://127.0.0.1:8787/**（或自定义端口）。脚本做三件事：编译缺失的 Rust release 二进制、构建缺失的前端 `web/dist`、用同一个进程同时提供 API 与界面。

### 方式 B：手动分步（单进程）

Rust 服务同时提供 API 和构建后的前端静态文件。

```bash
# 1) 构建内核 / 服务（首次编译需要几分钟）
cd server
PATH="$HOME/.cargo/bin:$PATH" cargo build --release

# 2) 构建前端
cd ../web
npm install
npm run build            # 产物在 web/dist

# 3) 启动（从仓库根目录）
cd ..
STATIC_DIR=web/dist PORT=8787 ./server/target/release/section-server
```

浏览器打开 **http://127.0.0.1:8787/**。页面启动后会自动：
1. 调用 `GET /api/health` 确认内核在线（右上角显示“内核就绪”）；
2. 载入内置示例 `fixtures/demo-building.json`（`POST /api/scene`）；
3. 在默认平面 `n=(0,0,1), d≈2.28` 处计算剖面（`POST /api/section`），右下结果区显示 6 条轮廓、4 个封口，且无诊断。

服务端会打印 `archisection kernel listening on http://0.0.0.0:8787` 与 `serving UI from web/dist`。

> 若容器内 Rust 链接失败（`/usr/local/bin/cc` 是入口包装器），仓库内已提供 `server/.cargo/config.toml` 将链接器固定为 `/usr/bin/gcc`；如仍报错，可显式设置 `CC=/usr/bin/gcc HOST_CC=/usr/bin/gcc`。

### 方式 C：开发模式（前端热更新）

```bash
# 终端 1：只跑内核服务
STATIC_DIR=web/dist PORT=8787 ./server/target/release/section-server

# 终端 2：Vite 开发服务器，/api 代理到 8787
cd web
npm run dev              # http://127.0.0.1:5173
```

---

## 2. 操作说明

| 区域 | 操作 |
| --- | --- |
| 视窗 | **左键拖拽青色平面**移动剖切位置（主交互，实时重算）；空白处拖拽旋转，滚轮缩放；“复位视角”回到初始视角 |
| 剖切平面 | Z / Y / X 轴或自定义法向；滑块与数值框设定平面常数 `d`；`计算剖面` 立即重算 |
| 显示 | 剖面轮廓与封口 / 剖切平面 / 选中时高亮**原面** / 拖拽实时计算；模型透明度 |
| 结果冻结 | 锁定当前剖面快照；之后移动平面会把冻结标记为“冻结已过期”并禁止导出 |
| 导出 | JSON / DXF / CSV，仅在已冻结且未过期时可用；文件内写明实际单位与平面定义 |

结果区三列：
- **轮廓**：每条轮廓的面积（m²）、所属组、对象、点数、孔→父轮廓关系；开放链标为“无面积（开放链）”。
- **诊断**：`emptySection`、`openChain`、`coplanarFaces`、`tangentContact` 等，附文字说明；没有问题时显示“没有诊断：剖面完整闭合。”
- **选中详情**：点击任一轮廓，显示全部交点（模型 XYZ）、来源面（对象·面号）、逐段来源，并可“聚焦此轮廓”或高亮原面。

顶点彩点：青色圆点是可拖拽的平面手柄。

---

## 3. 几何语义（判定可复现）

### 坐标系与单位
- 世界坐标为右手系、**Z 向上**，单位取自场景 `unit`（示例为 `meter`，`metersPerUnit = 1`）。
- 平面定义为 `dot(n, p) = d`，`n` 为单位法向，`d` 为世界坐标下的平面常数（与模型的单位一致）。
- 平面内基 `u × v = n`；二维坐标 `(u,v) = ((p - origin)·u, (p - origin)·v)`。结果里的 `plane.basisU/basisV/basisCrossesNormal` 和 `plane.definition` 会一并返回，导出文件也据此可复算。

### 非均匀缩放
每个对象带列主序 4×4 局部→世界矩阵。内核**先**把顶点变换到世界坐标，再用世界平面做相交判定；绝不会把世界平面参数套到局部顶点上。矩阵必须是仿射（拒绝投影矩阵）；`det < 0` 表示镜像，会交换三角形顶点顺序以恢复外法向。奇异 / 单轴塌缩的矩阵会被拒绝并给出诊断代码。

### 容差
线性容差由模型轴对齐包围盒的三维对角线导出：`tol = clamp(diag × 1e-7, 1e-9, 1e-2)`；角度容差 `1e-9`。结果 `tolerance.rules` 中列出的规则包括：
- 点在平面上：`|n·p − d| ≤ tol`；
- 跨越边按**排序后的端点**计算插值参数，使相邻三角形共享逐位相同的交点；
- 共面边按第三顶点所在侧去重，重合双面只输出一条轮廓；
- 掠射边仅在其两侧都有三角形时输出；
- 短于容差的线段丢弃、重合线段合并并合并来源；
- 环按外轮廓逆时针、孔洞顺时针统一方向；
- 断开的分量永不按最近端点拼接。

### 轮廓连接
节点按**拓扑与几何**双重键控：`(对象, 顶点枚举 | 规范化边对)`，并在容差内用空间哈希合并接缝。闭合环、开放链、孔洞分别表示；嵌套判定采用“最大优先 + 严格包含”，仅边界接触会标为 `contourTouchAmbiguous`。
平面恰好穿过网格边时会出现度 4 非流形节点：内核先为每个节点预算**对称的线段配对**（同组优先、再按直线度），再沿固定配对追踪，避免贪心追踪把闭合环错拆成开放链；非度 2 节点会写入 `nonManifoldJunction` 诊断。
开放网格留下的未闭合链会以开放链输出（红色虚线 + 两端圆点），**不伪造面积、不生成封口**，并写入 `openChain` 诊断。

### 结果标识与确定性
`result_id` 是规范化结果元组（场景、单位、平面、容差、轮廓、封口、诊断）的 FNV-1a 哈希；同样输入必得同样 ID，可作冻结/导出的唯一凭据。

---

## 4. HTTP API

### `GET /api/health`
```json
{ "ok": true, "service": "archisection", "kernel": "section_kernel", "version": "0.1.0" }
```

### `POST /api/scene`
请求体为场景文档（`archisection.scene/1`）。返回 `sceneId`、单位、对象摘要、包围盒与烘焙提示。场景保存在服务端，后续剖切只需传 `sceneId`。

### `POST /api/section`
```json
{
  "sceneId": "scene-…",          // 或直接内联 "scene": { … }
  "normal": [0, 0, 1],
  "offset": 1.5,
  "origin": [0, 0, 0],           // 可选：用平面上一点代替 offset
  "includeSegments": true
}
```
`normal` 可以为任意非零向量（内核会归一化）；零向量返回 `400 invalid_plane`。返回 `{ "ok": true, "result": { … } }`，或错误 `{ "ok": false, "error": { "code", "message", "details" } }`。

错误码：`invalid_json`、`missing_scene`、`unknown_scene`、`invalid_plane`、`invalid_unit`、`scene_invalid`（含逐对象 `details`）。

---

## 5. 导出内容

三种格式都写明**实际单位**和**平面定义**，不是截图：
- **JSON**（`archisection.export/1`）：单位、坐标系、平面、容差、摘要（封口面积、来源面数）、全部轮廓（含 `points2d/points3d/faceRefs/segments`）、封口与诊断。
- **DXF**：每条轮廓一个 `POLYLINE`，X=平面 u、Y=平面 v，`1000` 扩展数据带 `id` 与 `area_m2`；`999` 注释头写明单位、法向、`d`、原点、基向量与冻结时间。
- **CSV**：`#` 注释头写明单位与平面；字段含模型 XYZ、平面 u/v、区域、轮廓属性与封口行。

---

## 6. 内置测试场景

| 文件 | 覆盖点 |
| --- | --- |
| `fixtures/demo-building.json` | 双房间带内腔（孔洞）、非均匀缩放 0.12 的薄隔墙、30° 旋转塔楼、独立附属体、共面雨篷 |
| `fixtures/open-shell.json` | 缺一面墙的开放房间 + 完整对照体：平面切出开放链，保留诊断且不伪造面积 |
| `fixtures/edge-cases.json` | 平面恰好穿过立方体顶点/边的 `vertex-touch-box`，以及四块扇形三角片共面的 `coplanar-canopy`：临界输入必须保持闭合轮廓、共面片只输出一条外轮廓 |

内核回归测试（30 项 = 内核 17 + 示例场景 4 + 临界输入 4 + CLI 端到端 5，含带洞体、相切平面、薄墙、多分量、镜像、确定性、非流形度 4 节点配对追踪）：`cd server && cargo test`。
浏览器端到端测试（10 项，驱动真实内核与界面，含拖拽移动剖切平面后实时重算）：`cd web && npm run test:e2e`。

> 无图形界面的环境里运行 Playwright 需要补齐浏览器系统库；非 root 环境可把对应 deb 解包到本地目录后设置 `LD_LIBRARY_PATH`（无需写系统目录）。

截图证据见 `web/artifacts/`（默认剖面、拖拽后、选中来源高亮、冻结、开放链）。

---

## 7. 命令行（仅辅助图形界面）

**模型剖切的图形入口是第 1 节的浏览器视窗**（`./start.sh` 后打开打印的 URL）：在视窗里拖拽平面、查看轮廓与封口、冻结导出。下面的 CLI 只是把同一个内核用于后台批处理与发布流水线，不替代图形界面。

```bash
section-server serve [--port N] [--static-dir DIR]   # 启动图形视窗服务（无参数时默认即此命令）
section-server section --scene model.json --normal 0,0,1 --offset 1.5 --out result.json
section-server batch  --job job.json --out-dir DIR [--require-closed]
section-server verify --result result.json [--require-closed]
section-server lock   --repo-root . --out release/release-lock.json [--check]
```

- `section` / `batch` 的产物为原子写入（临时文件 + 改名）；剖切失败时**不会**留下结果文件，命令以非零码退出，失败产物无法进入发布目录。
- `batch` 读取 `archisection.batch/1` 任务文档，写出 `archisection.batch-report/1` 报告；每个成功结果附带 `archisection.result-manifest/1` 清单（resultId、SHA-256、字节数、平面、容差）。
- `verify` 会**重新加载**结果文件，校验法向单位化、轮廓/封口顶点在平面上、闭合环面积有限、封口引用的外轮廓存在且三角面非空；`--require-closed` 下开放链直接拒绝。
- `lock` 生成发布锁（`archisection.release-lock/1`），固化依赖版本、容差规则、验收标准与 fixtures 的 SHA-256；`--check` 发现漂移即以非零码退出。

批处理包装脚本：`scripts/batch.sh <job.json> [out-dir] [--require-closed]`（二进制缺失时自动构建）。

---

## 8. 发布流水线（构建、校验、原子发布）

`scripts/release.sh` 把**图形剖切视窗与后台批处理联合发布**到 `release/current/`，全程 `set -euo pipefail`，任何一步失败都在触碰发布目录前中止：

1. 冷启动：删除 `server/target/release` 与 `web/dist` 后重新构建二进制和前端；
2. 运行全部 30 项 Rust 测试（含穿顶点、共面三角片临界输入），不以默认值掩盖错误；
3. 重新生成发布锁并与已提交的 `release/release-lock.json` 做 `--check` 比对；
4–5. 在临时暂存目录跑临界输入与示例模型的后台批处理；
6. 对每个结果执行 `verify --require-closed`，并核对清单文件存在、哈希匹配（闭合轮廓与网格校验均对应真实执行证据）；
7. **失败闸门**：开放壳任务必须非零退出，且目录里只能留下 `batch-report.json`，证明失败剖切不会产生可用产物；
8. 组装 `bin/ ui/ fixtures/ jobs/ start.sh batch.sh`，在暂存目录内冷启动服务做 curl 冒烟（健康检查、首页与视窗 JS 包均可下载），通过后才把暂存目录**原子改名**为 `release/current`（旧版本保留为 `release/previous`）。

发布任务与测试模型锁定在仓库中：`release/jobs/*.batch.json`、`release/release-lock.json`、`fixtures/*.json`；生成物（`release/current`、`release/previous`、暂存目录、批处理输出）已在 `.gitignore` 中忽略。

---

## 9. 目录结构

```
server/            Rust 内核与 HTTP 服务
  src/math.rs        向量/平面/容差
  src/model.rs       场景解析、矩阵烘焙、校验
  src/section.rs     剖切内核（拓扑节点、配对追踪、嵌套、封口、诊断）
  src/cli.rs         辅助 CLI：section/batch/verify/lock，原子写入与发布锁
  src/main.rs        HTTP 启动（serve_http），向库内注入 serve 入口
  tests/             内核、示例场景、临界输入、CLI 端到端回归
  examples/          生成内置示例场景
web/               前端（Three.js + Vite）
  src/viewport.js    3D 视窗、平面拖拽、轮廓/封口/原面高亮
  src/main.js        控件与状态、冻结、失败反馈
  src/export.js      JSON/DXF/CSV 导出
  e2e/               Playwright 端到端测试
scripts/           release.sh（联合发布流水线）、batch.sh（批处理包装）
release/
  jobs/              已锁定的批处理任务（临界输入 / 示例 / 失败闸门）
  release-lock.json  依赖、容差规则、验收标准、fixture 哈希
  current/           流水线原子发布的当前版本（生成物，git 忽略）
fixtures/          示例与临界输入场景
```
