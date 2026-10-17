# 准备 cargo-deny 的公告库缓存：把 vendor/advisory-db 这份**钉住的快照**
# 变成 cargo-deny 期望的布局，之后的漏洞检查不再跟上游 main 走。
#
# 为什么需要这一步：cargo-deny 的 db-path 是「缓存根」而不是库本体 —— 它去
# <root>/<name>-<hash> 找库，并且（除非 --offline）先在这个目录里 git fetch 一次。
# 这里预先把它克隆成「origin 指向本地 submodule」的镜像，fetch 就只会原地打转，
# 不会把上游 main 的新通告拉进来。CI 在跑 cargo-deny 之前调用本脚本。
#
# 用法（在仓库任意位置都可以）：
#   pwsh -File scripts/prepare-advisory-db.ps1
#   cargo deny check advisories sources
#
# 目录名由 cargo-deny 对库 URL 做 XxHash64 得出（<URL 末段>-<哈希>）。cargo-deny
# 版本与 URL 都固定，所以这个名字稳定；万一升级 cargo-deny 后对不上，报错是
# "failed to find expected `crates` directory"，用
#   cargo-deny -L debug check advisories
# 看 "Opening advisory database at '<路径>'" 就能拿到新名字，改 $DbDir 即可。

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$DbDir = 'advisory-db-3157b0e258782691'
$repoRoot = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repoRoot 'vendor/advisory-db'

# 1. 快照必须在本地：submodule 没检出时给出可操作的提示，而不是让 cargo-deny
#    悄悄去连 RustSec（那样就退回成「跟随上游」了）。
if (-not (Test-Path (Join-Path $source '.git'))) {
    throw "找不到 $source —— 公告库快照还没检出。先跑：git submodule update --init vendor/advisory-db"
}
$pinned = (& git -C $source rev-parse HEAD 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $pinned) {
    throw "$source 不是有效的 git 仓库，无法取到钉住的提交。先跑：git submodule update --init vendor/advisory-db"
}

# 2. cargo-deny 的默认缓存根：$CARGO_HOME/advisory-dbs（缺省 ~/.cargo/advisory-dbs）。
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
$dbRoot = Join-Path $cargoHome 'advisory-dbs'
$dest = Join-Path $dbRoot $DbDir

# 3. 已经指向同一个提交就跳过 —— CI 每次跑、本地反复跑都是幂等的。
if (Test-Path $dest) {
    $current = (& git -C $dest rev-parse HEAD 2>$null)
    if ($LASTEXITCODE -eq 0 -and $current -eq $pinned) {
        Write-Output "advisory db 已就绪：$pinned"
        Write-Output "  $dest"
        exit 0
    }
    Remove-Item -Recurse -Force $dest
}

# 4. submodule 检出后 HEAD 是游离的，本地通常没有 main 分支；而 cargo-deny 会跑
#    `git fetch +main:main`，所以补一个指向钉住提交的 main ref。只动本地引用，
#    不改内容、不改提交，也不会动 submodule 的工作区。
& git -C $source update-ref refs/heads/main $pinned
if ($LASTEXITCODE -ne 0) { throw "在 $source 里建立 main 引用失败" }

# 5. 从 submodule 克隆镜像。--no-hardlinks 让两个仓库各自持有对象文件，
#    之后 cargo-deny 在镜像里 fetch/reset 都不会碰到 submodule。
New-Item -ItemType Directory -Force -Path $dbRoot | Out-Null
& git clone -q --no-hardlinks $source $dest
if ($LASTEXITCODE -ne 0 -or -not (Test-Path (Join-Path $dest '.git'))) {
    throw "从 $source 克隆公告库镜像到 $dest 失败"
}
if (-not (Test-Path (Join-Path $dest 'crates'))) {
    throw "$dest 里没有 crates/ —— 布局可能已随 cargo-deny 版本变化（见本脚本顶部说明）"
}

Write-Output "advisory db 就绪：$pinned"
Write-Output "  source: $source"
Write-Output "  cache : $dest"
