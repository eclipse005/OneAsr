# 让 Windows 本机的 `python3` 能跑 —— 本机开发环境的一次性适配，入口只有一个。
#
# 背景：微软商店的「应用执行别名」会在 %LOCALAPPDATA%\Microsoft\WindowsApps 里放一个
# python3.exe 占位程序，而 Python 官方安装器与 conda 都只装 python.exe。于是 `python3`
# 落到那个占位上：打印 "Python was not found..." 并以 9009 退出。仓库的文档、脚本提示
# 与 CI 里都只有 `python3` 一种拼写（CI 跑在 ubuntu 上，那里本来就是 python3），
# 与其在每个命令里改拼写，不如在本机补一个转发。
#
# 用法（幂等，跑几次都一样）：
#   pwsh -File scripts/ensure-python3.ps1
# 撤销：删掉脚本最后打印的那个 python3.cmd 即可，不改 PATH、不动注册表。
#
# 选解释器的规则：按 python → py -3 的顺序试，必须能 `import tomllib`（3.11+，
# scripts/verify-version.py 需要）。转发写成 `python %*` 这种按名字转的形式，
# 而不是写死绝对路径：conda 切了环境之后，`python` 会指向新环境，`python3` 跟着走。

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

if ($env:OS -ne 'Windows_NT') {
    Write-Output '非 Windows：python3 本来就是标准命令，无需处理。'
    exit 0
}

function Get-Python3Info([string]$exe, [string[]]$pre) {
    $code = 'import sys, tomllib; print(sys.version.split()[0])'
    $out = & $exe @($pre + @('-c', $code)) 2>&1
    if ($LASTEXITCODE -ne 0) { return $null }
    $ver = ($out | Where-Object { $_ -is [string] } | Select-Object -Last 1)
    if (-not $ver) { return $null }
    return $ver.Trim()
}

# 0. 已经能用就别动它（含上一次跑本脚本留下的转发）。
$existing = Get-Command python3 -ErrorAction SilentlyContinue
if ($existing) {
    $ver = Get-Python3Info 'python3' @()
    if ($ver) {
        Write-Output "python3 已可用：$ver（$($existing.Source)），不需要处理。"
        exit 0
    }
    Write-Output "python3 指向 $($existing.Source)，但它跑不起来（多半是商店别名），继续找真正的解释器。"
}

# 1. 找一个 3.11+ 的解释器。tomllib 从 3.11 才有，缺了它版本门禁会直接失败。
$candidate = $null
foreach ($c in @(
        @{ Label = 'python'; Forward = 'python'; Exe = 'python'; Pre = @() },
        @{ Label = 'py -3'; Forward = 'py -3'; Exe = 'py'; Pre = @('-3') }
    )) {
    $ver = Get-Python3Info $c.Exe $c.Pre
    if ($ver) {
        Write-Output "找到可用解释器：$($c.Label) → $ver"
        $candidate = $c
        break
    }
    Write-Output "跳过 $($c.Label)：跑不通，或低于 3.11（缺 tomllib）。"
}
if (-not $candidate) {
    throw '没找到 Python 3.11+。先装一个（conda 或 python.org），确认 python 或 py -3 能跑，再回来执行本脚本。'
}

# 2. 把转发放在解释器所在目录：它在 PATH 里排在商店别名（WindowsApps）之前，
#    所以 python3 会先命中这里。内容按名字转发，不是写死路径。
$dir = $null
foreach ($cmd in @('python', 'py')) {
    $found = Get-Command $cmd -ErrorAction SilentlyContinue
    if ($found -and $found.Source) { $dir = Split-Path -Parent $found.Source; break }
}
if (-not $dir) { $dir = Split-Path -Parent ((Get-Command python).Source) }
$shim = Join-Path $dir 'python3.cmd'
$content = "@echo off`r`n$($candidate.Forward) %*`r`n"

if (Test-Path $shim) {
    $old = [IO.File]::ReadAllText($shim)
    if ($old -eq $content) {
        Write-Output "转发已存在且内容一致：$shim"
    } else {
        $bak = "$shim.bak-$([DateTime]::Now.ToString('yyyyMMddHHmmss'))"
        Move-Item $shim $bak
        [IO.File]::WriteAllText($shim, $content, (New-Object Text.UTF8Encoding($false)))
        Write-Output "已覆盖原有 $shim（旧文件备份到 $bak）"
    }
} else {
    [IO.File]::WriteAllText($shim, $content, (New-Object Text.UTF8Encoding($false)))
    Write-Output "已写入转发：$shim"
}

# 3. 用一个新进程验证（当前进程可能缓存了"python3 不存在"）。
$check = & cmd.exe /c 'python3 --version' 2>&1
if ($LASTEXITCODE -ne 0) {
    throw "转发写好了但 python3 仍然跑不通，检查 $shim 的内容：$check"
}
Write-Output "python3 现在可用：$check"
Write-Output "撤销：Remove-Item '$shim'"
