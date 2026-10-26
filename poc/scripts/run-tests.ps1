<#
.SYNOPSIS
  Phase 4 POC test runner. Each test writes logs, stats (JSON lines) and a
  summary.json with named checks to out\results\<run>\<test>\.

.EXAMPLE
  .\scripts\run-tests.ps1                       # all automated tests, 30 min sustained run
  .\scripts\run-tests.ps1 -Tests start-stop,network -SustainMin 5
  .\scripts\run-tests.ps1 -Tests device-removal  # interactive: asks you to unplug the camera

.NOTES
  Requires: release build (cargo build --release), MediaMTX (JK_MEDIAMTX) and an
  ffmpeg/ffprobe on PATH used only as external validation tools (decode checks,
  stream pull). Nothing here links or ships those tools.
#>
param(
    [string[]]$Tests = @('devices', 'start-stop', 'invalid-input', 'encoder-errors', 'simultaneous', 'network', 'leak-cycles', 'software-baseline', 'sustained'),
    [int]$SustainMin = 30,
    [int]$LeakCycles = 25,
    [string]$Run = (Get-Date -Format 'yyyyMMdd-HHmmss')
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path $PSScriptRoot -Parent
Set-Location $Root
. "$PSScriptRoot\env.ps1" | Out-Null
$Bin = Join-Path $Root 'target\release'
$Engine = Join-Path $Bin 'jalakam-engine-poc.exe'
$Ctl = Join-Path $Bin 'jkctl.exe'
$Preview = Join-Path $Bin 'jalakam-preview-poc.exe'
. "$PSScriptRoot\win.ps1"
$ResultsRoot = Join-Path $Root "out\results\$Run"
New-Item -ItemType Directory -Force $ResultsRoot | Out-Null
$StreamUrl = 'rtmp://127.0.0.1:1935/live/test'
$script:Summary = @{}

# ------------------------------------------------------------------ helpers

function Start-Mtx([string]$Dir) {
    Stop-Mtx
    $script:Mtx = Start-Process -FilePath $env:JK_MEDIAMTX -ArgumentList (Join-Path $Root 'config\mediamtx.yml') -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $Dir "mediamtx-$(Get-Date -Format HHmmss).log") -RedirectStandardError (Join-Path $Dir 'mediamtx.err')
    Wait-Until { $c = New-Object Net.Sockets.TcpClient; try { $c.Connect('127.0.0.1', 1935); $true } finally { $c.Dispose() } } 10 0.2 | Out-Null
}
function Stop-Mtx {
    Get-Process mediamtx -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 300
}

function Start-Engine([string]$Dir, [string]$Config, [string[]]$Extra = @()) {
    Get-Process jalakam-engine-poc -ErrorAction SilentlyContinue | Stop-Process -Force
    $argList = @('run', '--config', $Config, '--stats-file', (Join-Path $Dir 'stats.jsonl')) + $Extra
    $p = Start-Process -FilePath $Engine -ArgumentList $argList -PassThru -NoNewWindow `
        -RedirectStandardError (Join-Path $Dir 'engine.log') -RedirectStandardOutput (Join-Path $Dir 'engine.out')
    # Ready when the control pipe answers.
    if (-not (Wait-Until { (Jk hello).ok } 15)) { throw "engine did not come up (see $Dir\engine.log)" }
    $p
}

function Stop-Engine($Proc, [int]$TimeoutS = 20) {
    Jk shutdown | Out-Null
    $ok = $Proc.WaitForExit($TimeoutS * 1000)
    if (-not $ok) { $Proc | Stop-Process -Force }
    [pscustomobject]@{ exited = $ok; exit_code = $(if ($ok) { $Proc.ExitCode } else { $null }) }
}

# Calls jkctl; returns @{ ok; result | error; message }.
function Jk([string]$Cmd, [string]$Arg) {
    $a = @($Cmd); if ($Arg) { $a += $Arg }
    $out = & $Ctl @a --wait-s 1 2>$null | Out-String
    $code = $LASTEXITCODE
    try { $j = $out | ConvertFrom-Json } catch { $j = $null }
    if ($code -eq 0) { @{ ok = $true; result = $j } }
    elseif ($j) { @{ ok = $false; error = $j.error; message = $j.message } }
    else { @{ ok = $false; error = 'NO_RESPONSE'; message = $out.Trim() } }
}
function Stats { (Jk stats).result }
function Output($s, $name) { $s.outputs | Where-Object name -eq $name }
# Stopped outputs stay in stats with state 'idle'.
function Test-Idle($s, $name) { $o = Output $s $name; (-not $o) -or $o.status.state -eq 'idle' }
function Encoder($s, $name) { $s.encoders | Where-Object name -eq $name }

function Wait-Until([scriptblock]$Cond, [double]$TimeoutS, [double]$StepS = 0.5) {
    $end = (Get-Date).AddSeconds($TimeoutS)
    while ((Get-Date) -lt $end) {
        try { if (& $Cond) { return $true } } catch {}
        Start-Sleep -Milliseconds ([int]($StepS * 1000))
    }
    $false
}

# Decodes every frame of a media file with the external ffmpeg.
function Test-Decode([string]$File) {
    if (-not (Test-Path $File)) { return [pscustomobject]@{ file = $File; ok = $false; reason = 'missing' } }
    $err = & ffmpeg -v error -i $File -f null - 2>&1 | Out-String
    $probe = & ffprobe -v error -show_entries 'format=duration,size:stream=codec_type,codec_name,width,height,avg_frame_rate,sample_rate,channels' -count_packets -show_entries 'stream=nb_read_packets' -of json $File | ConvertFrom-Json
    $v = $probe.streams | Where-Object codec_type -eq 'video' | Select-Object -First 1
    $a = $probe.streams | Where-Object codec_type -eq 'audio' | Select-Object -First 1
    [pscustomobject]@{
        file          = (Split-Path $File -Leaf)
        ok            = ($err.Trim().Length -eq 0 -and $v -and $a)
        decode_errors = $err.Trim()
        duration_s    = [double]$probe.format.duration
        size_mb       = [math]::Round([double]$probe.format.size / 1MB, 2)
        video         = if ($v) { "$($v.codec_name) $($v.width)x$($v.height) $($v.avg_frame_rate) packets $($v.nb_read_packets)" }
        audio         = if ($a) { "$($a.codec_name) $($a.sample_rate) Hz $($a.channels) ch packets $($a.nb_read_packets)" }
    }
}

# Pulls the live stream from the local server for N seconds (a player's view).
# Retries like a player would: the server lists the path only after its own
# track detection (about 2 s after the first keyframe with MediaMTX).
function Get-StreamSample([string]$Dir, [int]$Seconds, [string]$Name = 'pull') {
    $file = Join-Path $Dir "$Name.flv"
    $log = Join-Path $Dir "$Name.ffmpeg.log"
    foreach ($try in 1..5) {
        "--- attempt $try $(Get-Date -Format o)" | Out-File $log -Append
        & ffmpeg -v error -y -rw_timeout 10000000 -i $StreamUrl -t $Seconds -c copy $file 2>&1 | Out-File $log -Append
        if ($LASTEXITCODE -eq 0 -and (Test-Path $file)) { break }
        Start-Sleep 2
    }
    $r = Test-Decode $file
    $r | Add-Member pull_attempts $try
    $r
}

# Theil-Sen slope (median of pairwise slopes): one allocator or driver cache
# step does not read as a per-cycle leak, a steady climb still does.
function Get-RobustSlope([double[]]$y) {
    $sl = [System.Collections.Generic.List[double]]::new()
    for ($i = 0; $i -lt $y.Count; $i++) { for ($j = $i + 1; $j -lt $y.Count; $j++) { $sl.Add(($y[$j] - $y[$i]) / ($j - $i)) } }
    if ($sl.Count -eq 0) { return 0 }
    $a = $sl.ToArray(); [array]::Sort($a)
    [math]::Round($a[[int][math]::Floor($a.Count / 2)], 3)
}

# Least-squares slope of y over the sample index.
function Get-Slope([double[]]$y) {
    $n = $y.Count; if ($n -lt 2) { return 0 }
    $mx = ($n - 1) / 2; $my = ($y | Measure-Object -Average).Average
    $num = 0; $den = 0
    for ($i = 0; $i -lt $n; $i++) { $num += ($i - $mx) * ($y[$i] - $my); $den += ($i - $mx) * ($i - $mx) }
    [math]::Round($num / $den, 3)
}

function Read-Stats([string]$Dir) {
    $f = Join-Path $Dir 'stats.jsonl'
    if (-not (Test-Path $f)) { return @() }
    Get-Content $f | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json }
}

function Pct([double[]]$v, [double]$p) {
    if (-not $v -or $v.Count -eq 0) { return $null }
    $s = $v | Sort-Object
    $s[[math]::Min($s.Count - 1, [math]::Ceiling($p / 100 * $s.Count) - 1)]
}

# Summarises a stats file against the budgets in performance-budgets.md.
function Measure-Stats([string]$Dir, [int]$SkipFirstS = 5) {
    $all = @(Read-Stats $Dir)
    $st = @($all | Where-Object { $_.uptime_s -ge $SkipFirstS })
    if ($st.Count -lt 2) { return $null }
    $first = $st[0]; $last = $all[-1]
    $ticks = $last.render.ticks - $first.render.ticks
    $lagged = $last.render.lagged - $first.render.lagged
    $p = $st | ForEach-Object { $_.process }
    $encDrops = 0; foreach ($e in $last.encoders) { $encDrops += $e.dropped_queue_full }
    [ordered]@{
        samples                      = $st.Count
        duration_s                   = [math]::Round($last.uptime_s - $first.uptime_s, 1)
        render_ticks                 = $ticks
        render_lagged                = $lagged
        render_lagged_pct            = if ($ticks) { [math]::Round(100 * $lagged / ($ticks + $lagged), 4) } else { $null }
        preview_skipped              = $last.render.preview_skipped
        tick_ms_p99_10s_max          = ($st | ForEach-Object { $_.render.tick_ms_p99_10s } | Measure-Object -Maximum).Maximum
        jitter_ms_p99_10s_max        = ($st | ForEach-Object { $_.render.jitter_ms_p99_10s } | Measure-Object -Maximum).Maximum
        jitter_windows_over_1ms_pct  = [math]::Round(100.0 * @($st | Where-Object { $_.render.jitter_ms_p99_10s -gt 1.0 }).Count / [math]::Max(1, @($st).Count), 1)
        tick_windows_over_2ms_pct    = [math]::Round(100.0 * @($st | Where-Object { $_.render.tick_ms_p99_10s -gt 2.0 }).Count / [math]::Max(1, @($st).Count), 1)
        tick_ms_mean                 = [math]::Round($last.render.tick_ms_avg, 3)
        # Per-stage GPU time from timestamp queries (performance-budgets.md §1).
        gpu_composite_preview_ms_mean = [math]::Round(($st | Measure-Object { $_.render.gpu_composite_ms_mean_10s } -Average).Average, 3)
        gpu_composite_preview_ms_p95_max = ($st | ForEach-Object { $_.render.gpu_composite_ms_p95_10s } | Measure-Object -Maximum).Maximum
        # VideoProcessorBlt is invisible to 3D-queue timestamps. On the RTX 4050
        # test machine PDH VideoProcessing and Copy also read 0 for it, and an
        # A/B run (canvas only vs. + 1080p encoder) shows no 3D increase, so the
        # conversion cost is not isolated here. The budget check uses the total
        # per-process 3D + VideoProcessing time instead (gpu_engine_ms_per_frame_pdh).
        gpu_convert_ms_timestamps     = [math]::Round(($st | Measure-Object { $_.render.gpu_convert_ms_mean_10s } -Average).Average, 3)
        gpu_video_processing_pct_avg  = [math]::Round(($st | ForEach-Object { $_.process.gpu_video_processing_pct } | Measure-Object -Average).Average, 2)
        gpu_convert_ms_per_frame_pdh  = [math]::Round((($st | ForEach-Object { $_.process.gpu_video_processing_pct } | Measure-Object -Average).Average / 100) * 1000 / $last.render.fps_target, 3)
        gpu_copy_pct_avg              = [math]::Round(($st | ForEach-Object { $_.process.gpu_copy_pct } | Measure-Object -Average).Average, 2)
        # Timeline span of the whole tick: includes CPU submission gaps and
        # other processes' GPU work interleaved with ours. Not a cost figure.
        gpu_tick_span_ms_avg          = [math]::Round($last.render.gpu_ms_avg, 3)
        gpu_tick_span_ms_p95_max      = ($st | ForEach-Object { $_.render.gpu_ms_p95_10s } | Measure-Object -Maximum).Maximum
        # PDH per-process 3D engine busy time, converted to ms per canvas frame.
        gpu_3d_ms_per_frame_pdh       = [math]::Round((($st | ForEach-Object { $_.process.gpu_3d_pct } | Measure-Object -Average).Average / 100) * 1000 / $last.render.fps_target, 3)
        cpu_cores_avg                = [math]::Round((($p | Measure-Object cpu_pct_of_one_core -Average).Average) / 100, 3)
        cpu_cores_p95                = [math]::Round((Pct ($p | ForEach-Object cpu_pct_of_one_core) 95) / 100, 3)
        cpu_machine_pct_avg          = [math]::Round(($p | Measure-Object cpu_pct_of_machine -Average).Average, 2)
        working_set_mb_first_last    = "$([math]::Round($first.process.working_set_mb,1)) -> $([math]::Round($last.process.working_set_mb,1))"
        private_mb_first_last        = "$([math]::Round($first.process.private_mb,1)) -> $([math]::Round($last.process.private_mb,1))"
        private_mb_growth_pct        = [math]::Round(100 * ($last.process.private_mb - $first.process.private_mb) / $first.process.private_mb, 2)
        handles_first_last           = "$($first.process.handles) -> $($last.process.handles)"
        threads_first_last           = "$($first.process.threads) -> $($last.process.threads)"
        gpu_3d_pct_avg               = [math]::Round(($p | Measure-Object gpu_3d_pct -Average).Average, 1)
        gpu_video_encode_pct_avg     = [math]::Round(($p | Measure-Object gpu_video_encode_pct -Average).Average, 1)
        gpu_dedicated_mb_max         = [math]::Round(($p | Measure-Object gpu_dedicated_mb -Maximum).Maximum, 1)
        encoders                     = @($last.encoders | ForEach-Object { "$($_.name)=$($_.codec) hw=$($_.hardware) zero_copy=$($_.zero_copy) in=$($_.frames_in) out=$($_.packets_out) drop=$($_.dropped_queue_full) enc_ms=$([math]::Round($_.encode_ms_avg,3)) state=$($_.status.state)" })
        encoder_queue_drops          = $encDrops
        outputs                      = @($last.outputs | ForEach-Object { "$($_.name) state=$($_.status.state) kbps=$([math]::Round($_.kbps)) drop=$($_.dropped_packets) reconnects=$($_.reconnects)" })
        audio                        = "$($last.audio.device): blocks $($last.audio.blocks_captured) underruns $($last.audio.underruns) overruns $($last.audio.overruns) drift_ms $($last.audio.drift_ms)"
        camera                       = "$($last.camera.name) $($last.camera.status.state) $($last.camera.format) frames $($last.camera.frames)"
    }
}

function Get-ShutdownLine([string]$Dir) {
    (Select-String -Path (Join-Path $Dir 'engine.log') -Pattern 'shutdown in' | Select-Object -Last 1).Line
}

function Test-CleanShutdown([string]$Dir, $Stop) {
    $line = Get-ShutdownLine $Dir
    $leak = Select-String -Path (Join-Path $Dir 'engine.log') -Pattern 'SHUTDOWN.THREAD_LEAK' -Quiet
    [ordered]@{ exited = $Stop.exited; exit_code = $Stop.exit_code; no_thread_leak = (-not $leak); line = $line }
}

function New-TestDir([string]$Name) {
    $d = Join-Path $ResultsRoot $Name
    New-Item -ItemType Directory -Force $d | Out-Null
    $d
}

function Save-Result([string]$Name, [string]$Dir, $Checks, $Data) {
    $pass = @($Checks.Values | Where-Object { $_ -ne $true }).Count -eq 0
    $r = [ordered]@{ test = $Name; pass = $pass; checks = $Checks; data = $Data }
    $r | ConvertTo-Json -Depth 8 | Out-File (Join-Path $Dir 'summary.json') -Encoding utf8
    $script:Summary[$Name] = $pass
    $failed = ($Checks.GetEnumerator() | Where-Object { $_.Value -ne $true } | ForEach-Object Key) -join ', '
    Write-Host ("[{0}] {1}{2}" -f $(if ($pass) { 'PASS' } else { 'FAIL' }), $Name, $(if ($failed) { " (failed: $failed)" } else { '' }))
}

function Get-RecordingPath($r) { if ($r -and $r.ok) { $r.result.path } }

# -------------------------------------------------------------------- tests

function Test-Devices {
    $d = New-TestDir 'devices'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & $Engine devices 2>(Join-Path $d 'devices.stderr.txt') | Out-File (Join-Path $d 'devices.json') -Encoding utf8
    $ms = $sw.ElapsedMilliseconds
    $j = Get-Content (Join-Path $d 'devices.json') -Raw | ConvertFrom-Json
    $checks = [ordered]@{
        has_adapters      = ($j.adapters.Count -gt 0)
        has_monitors      = ($j.monitors.Count -gt 0)
        has_camera        = ($j.cameras.Count -gt 0)
        has_capture_audio = (@($j.audio_endpoints | Where-Object flow -eq 'capture').Count -gt 0)
        has_encoder       = (@($j.encoders | Where-Object available).Count -gt 0)
        probe_under_5s    = ($ms -le 5000)  # ONB-002
    }
    Save-Result 'devices' $d $checks ([ordered]@{ probe_ms = $ms; encoders = $j.encoders; capture_methods = $j.capture_methods })
}

function Test-StartStop {
    $d = New-TestDir 'start-stop'
    Start-Mtx $d
    $eng = Start-Engine $d 'config\baseline.toml'
    $cycles = @(); $recs = @()
    foreach ($i in 1..3) {
        $t0 = Get-Date
        $s1 = Jk stream-start; $r1 = Jk record-start
        $active = Wait-Until { $s = Stats; (Output $s 'stream').status.state -eq 'active' -and (Output $s 'record').status.state -eq 'active' } 15
        $dupe = Jk stream-start
        Start-Sleep 5
        $r2 = Jk record-stop; $s2 = Jk stream-stop
        $idle = Wait-Until { $s = Stats; (Test-Idle $s 'stream') -and (Test-Idle $s 'record') } 5
        $notActive = Jk stream-stop
        $rec = Get-RecordingPath $r2
        $dec = if ($rec) { Test-Decode $rec } else { $null }
        $recs += $dec
        $cycles += [ordered]@{
            cycle = $i; start_ok = ($s1.ok -and $r1.ok); active_within_15s = $active
            start_to_active_s = [math]::Round(((Get-Date) - $t0).TotalSeconds - 5, 1)
            duplicate_start_error = $dupe.error; stop_ok = ($r2.ok -and $s2.ok); idle_after_stop = $idle
            stop_when_idle_error = $notActive.error
            stream_stop_ms = $s2.result.stopped_in_ms; record_stop_ms = $r2.result.stopped_in_ms
            recording = $dec
        }
    }
    $stop1 = Stop-Engine $eng
    $clean1 = Test-CleanShutdown $d $stop1
    # Process restart: the pipe and preview names must be reusable at once.
    Copy-Item (Join-Path $d 'engine.log') (Join-Path $d 'engine-run1.log')
    $eng2 = Start-Engine $d 'config\baseline.toml' @('--stream', '--record', '--duration-s', '8')
    $restartOk = $eng2.WaitForExit(30000)
    $clean2 = Test-CleanShutdown $d ([pscustomobject]@{ exited = $restartOk; exit_code = $eng2.ExitCode })
    $rec2 = (Select-String -Path (Join-Path $d 'engine.log') -Pattern 'last recording: (.*)$' | Select-Object -Last 1).Matches.Groups[1].Value
    $dec2 = if ($rec2) { Test-Decode $rec2 }
    $checks = [ordered]@{
        all_starts_ok          = -not ($cycles | Where-Object { -not $_.start_ok })
        all_active             = -not ($cycles | Where-Object { -not $_.active_within_15s })
        duplicate_start_refused = -not ($cycles | Where-Object { $_.duplicate_start_error -ne 'OUT.ALREADY_ACTIVE' })
        all_stops_ok           = -not ($cycles | Where-Object { -not $_.stop_ok -or -not $_.idle_after_stop })
        stop_when_idle_refused = -not ($cycles | Where-Object { $_.stop_when_idle_error -ne 'OUT.NOT_ACTIVE' })
        recordings_decode      = -not ($recs | Where-Object { -not $_.ok })
        clean_shutdown         = ($clean1.exited -and $clean1.exit_code -eq 0 -and $clean1.no_thread_leak)
        restart_runs_and_exits = ($clean2.exited -and $clean2.exit_code -eq 0 -and $clean2.no_thread_leak)
        restart_recording_ok   = [bool]($dec2 -and $dec2.ok)
    }
    Save-Result 'start-stop' $d $checks ([ordered]@{ cycles = $cycles; shutdown = $clean1; restart = $clean2; restart_recording = $dec2 })
}

function Test-InvalidInput {
    $d = New-TestDir 'invalid-input'
    $eng = Start-Engine $d 'config\invalid-devices.toml' @('--record')
    Start-Sleep 6
    $s = Stats
    $camErr = $s.camera.status; $micErr = $s.audio.status
    $recActive = (Output $s 'record').status.state -eq 'active'
    # Runtime selection of a missing device, then recovery to a real one.
    $bad = Jk camera 'Definitely Missing Camera'
    $badSeen = Wait-Until { (Stats).camera.status.code -eq 'CAP.CAMERA.NOT_FOUND' } 6
    $good = Jk camera 'Insta360'
    $goodSeen = Wait-Until { (Stats).camera.status.state -eq 'active' } 10
    $micGood = Jk mic 'Insta360'
    $micSeen = Wait-Until { (Stats).audio.status.state -eq 'active' } 6
    # Malformed request straight on the pipe.
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream('.', 'jalakam-poc-engine', [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(2000); $w = New-Object System.IO.StreamWriter($pipe); $rd = New-Object System.IO.StreamReader($pipe)
    $w.AutoFlush = $true; $w.WriteLine('{"id":7,"cmd":"no_such_command"}'); $malformed = $rd.ReadLine() | ConvertFrom-Json; $pipe.Dispose()
    $r = Jk record-stop
    $dec = Test-Decode (Get-RecordingPath $r)
    $stop = Stop-Engine $eng
    $clean = Test-CleanShutdown $d $stop
    $checks = [ordered]@{
        camera_not_found_reported   = ($camErr.code -eq 'CAP.CAMERA.NOT_FOUND' -and $camErr.state -eq 'failed')
        mic_not_found_reported      = ($micErr.code -eq 'CAP.AUDIO.NOT_FOUND')
        engine_keeps_recording      = $recActive
        runtime_bad_camera_reported = ($bad.ok -and $badSeen)
        runtime_recovers_camera     = ($good.ok -and $goodSeen)
        runtime_recovers_mic        = ($micGood.ok -and $micSeen)
        malformed_request_rejected  = ($malformed.status -eq 'error' -and $malformed.code -eq 'CTRL.BAD_REQUEST')
        recording_decodes           = $dec.ok
        clean_shutdown              = ($clean.exited -and $clean.exit_code -eq 0 -and $clean.no_thread_leak)
    }
    Save-Result 'invalid-input' $d $checks ([ordered]@{ camera_status = $camErr; mic_status = $micErr; malformed = $malformed; recording = $dec; shutdown = $clean })
}

function Test-EncoderErrors {
    $d = New-TestDir 'encoder-errors'
    Start-Mtx $d
    # No fallback: start must fail with an encoder error code and leave the engine usable.
    $eng = Start-Engine $d 'config\encoder-unavailable.toml'
    $fail = Jk stream-start
    $s = Stats
    $stop1 = Stop-Engine $eng
    Copy-Item (Join-Path $d 'engine.log') (Join-Path $d 'engine-unavailable.log')
    # With fallback: start succeeds on the fallback and reports ENC.FALLBACK.
    $eng = Start-Engine $d 'config\encoder-fallback.toml'
    $ok = Jk stream-start
    Start-Sleep 6
    $s2 = Stats
    $enc = Encoder $s2 'stream'
    $pull = Get-StreamSample $d 4 'fallback-pull'
    Jk stream-stop | Out-Null
    $stop2 = Stop-Engine $eng
    $clean = Test-CleanShutdown $d $stop2
    $checks = [ordered]@{
        unavailable_start_refused = ((-not $fail.ok) -and $fail.error -like 'ENC.*')
        no_output_left_behind     = (Test-Idle $s 'stream')
        unavailable_clean_exit    = ($stop1.exited -and $stop1.exit_code -eq 0)
        fallback_started          = $ok.ok
        fallback_reported         = ($enc.codec -eq 'h264_mf' -and $enc.status.code -eq 'ENC.FALLBACK')
        fallback_stream_decodes   = $pull.ok
        clean_shutdown            = ($clean.exited -and $clean.exit_code -eq 0 -and $clean.no_thread_leak)
    }
    Save-Result 'encoder-errors' $d $checks ([ordered]@{ unavailable = $fail; fallback_encoder = $enc; pull = $pull })
}

function Test-Simultaneous {
    $d = New-TestDir 'simultaneous'
    Start-Mtx $d
    $eng = Start-Engine $d 'config\baseline.toml' @('--stream', '--record')
    Start-Sleep 5
    $pull = Get-StreamSample $d 20 'pull'
    Start-Sleep 2
    $r = Jk record-stop; Jk stream-stop | Out-Null
    $rec = Test-Decode (Get-RecordingPath $r)
    $m = Measure-Stats $d 5
    $stop = Stop-Engine $eng
    $clean = Test-CleanShutdown $d $stop
    $checks = [ordered]@{
        recording_decodes     = $rec.ok
        stream_decodes        = $pull.ok
        stream_is_720p        = ($pull.video -like '*1280x720*')
        recording_is_1080p    = ($rec.video -like '*1920x1080*')
        no_encoder_drops      = ($m.encoder_queue_drops -eq 0)
        clean_shutdown        = ($clean.exited -and $clean.exit_code -eq 0 -and $clean.no_thread_leak)
    }
    Save-Result 'simultaneous' $d $checks ([ordered]@{ recording = $rec; stream_pull = $pull; stats = $m; shutdown = $clean })
}

function Test-Network {
    $d = New-TestDir 'network'
    Start-Mtx $d
    $eng = Start-Engine $d 'config\baseline.toml' @('--stream', '--record')
    $up = Wait-Until { (Output (Stats) 'stream').status.state -eq 'active' } 15
    Start-Sleep 5
    $before = Stats
    $tLoss = Get-Date
    Stop-Mtx                                   # server goes away mid-stream
    $detected = Wait-Until { (Output (Stats) 'stream').status.state -eq 'reconnecting' } 15 0.2
    $detectS = [math]::Round(((Get-Date) - $tLoss).TotalSeconds, 1)
    $during = Stats
    Start-Sleep 10
    $during2 = Stats
    $recBytesGrow = (Output $during2 'record').bytes -gt (Output $during 'record').bytes
    $tBack = Get-Date
    Start-Mtx $d                               # server returns
    $recovered = Wait-Until { $o = Output (Stats) 'stream'; $o.status.state -eq 'active' -and $o.reconnects -ge 1 } 45
    $recoverS = [math]::Round(((Get-Date) - $tBack).TotalSeconds, 1)
    $pull = Get-StreamSample $d 6 'after-recovery'
    $after = Stats
    $r = Jk record-stop; Jk stream-stop | Out-Null
    $rec = Test-Decode (Get-RecordingPath $r)
    $stop = Stop-Engine $eng
    $clean = Test-CleanShutdown $d $stop
    $checks = [ordered]@{
        stream_up                  = $up
        loss_detected              = $detected
        loss_has_error_code        = ((Output $during2 'stream').status.code -like 'OUT.RTMP.*')
        recording_unaffected       = ($recBytesGrow -and (Output $during2 'record').status.state -eq 'active')
        stream_recovered           = $recovered
        stream_decodes_after       = $pull.ok
        recording_decodes          = $rec.ok
        clean_shutdown             = ($clean.exited -and $clean.exit_code -eq 0 -and $clean.no_thread_leak)
    }
    Save-Result 'network' $d $checks ([ordered]@{
        loss_detect_s = $detectS; recover_s_after_server_back = $recoverS
        stream_during_loss = (Output $during2 'stream'); stream_after = (Output $after 'stream')
        record_after = (Output $after 'record'); pull = $pull; recording = $rec; shutdown = $clean
    })
}

function Test-LeakCycles {
    $d = New-TestDir 'leak-cycles'
    Start-Mtx $d
    $eng = Start-Engine $d 'config\baseline.toml'
    Start-Sleep 3
    $samples = [System.Collections.Generic.List[object]]::new()
    $sample = { param($kind, $i) $p = (Stats).process; [pscustomobject]@{ kind = $kind; cycle = $i; handles = $p.handles; threads = $p.threads; private_mb = [math]::Round($p.private_mb, 1); gpu_mb = [math]::Round($p.gpu_dedicated_mb, 1) } }
    $samples.Add((& $sample 'baseline' 0))
    $N = $LeakCycles
    foreach ($i in 1..$N) {
        Jk stream-start | Out-Null; Jk record-start | Out-Null
        Start-Sleep 3
        Jk record-stop | Out-Null; Jk stream-stop | Out-Null
        Start-Sleep 1
        $samples.Add((& $sample 'output' $i))
    }
    foreach ($i in 1..$N) {
        Jk camera 'Animaze' | Out-Null; Start-Sleep 2
        Jk camera 'Insta360' | Out-Null; Start-Sleep 3
        $samples.Add((& $sample 'camera' $i))
    }
    foreach ($i in 1..10) {
        Jk mic 'Microphone Array' | Out-Null; Start-Sleep 1
        Jk mic 'Insta360' | Out-Null; Start-Sleep 1
        $samples.Add((& $sample 'mic' $i))
    }
    $stop = Stop-Engine $eng
    $clean = Test-CleanShutdown $d $stop
    $samples | ConvertTo-Json | Out-File (Join-Path $d 'samples.json')
    # Slope per cycle after a 5-cycle warm-up (MF, WASAPI and the driver grow
    # caches on first use). A real per-cycle leak gives a slope >= 1.
    $growth = [ordered]@{}
    foreach ($k in 'output', 'camera', 'mic') {
        $s = @($samples | Where-Object { $_.kind -eq $k -and $_.cycle -gt 5 })
        $growth["${k}_handles_per_cycle"] = Get-RobustSlope ($s | ForEach-Object handles)
        $growth["${k}_threads_per_cycle"] = Get-RobustSlope ($s | ForEach-Object threads)
        $growth["${k}_private_mb_per_cycle"] = Get-RobustSlope ($s | ForEach-Object private_mb)
        $growth["${k}_private_mb_per_cycle_lsq"] = Get-Slope ($s | ForEach-Object private_mb)
        $growth["${k}_private_mb_first_last"] = "$($s[0].private_mb) -> $($s[-1].private_mb)"
        $growth["${k}_handles_first_last"] = "$($s[0].handles) -> $($s[-1].handles)"
    }
    $checks = [ordered]@{
        output_no_handle_leak = ($growth.output_handles_per_cycle -lt 0.5)
        output_no_thread_leak = ($growth.output_threads_per_cycle -lt 0.2)
        output_no_memory_leak = ($growth.output_private_mb_per_cycle -lt 0.5)
        camera_no_handle_leak = ($growth.camera_handles_per_cycle -lt 0.5)
        camera_no_thread_leak = ($growth.camera_threads_per_cycle -lt 0.2)
        camera_no_memory_leak = ($growth.camera_private_mb_per_cycle -lt 0.5)
        mic_no_handle_leak    = ($growth.mic_handles_per_cycle -lt 0.5)
        clean_shutdown        = ($clean.exited -and $clean.exit_code -eq 0 -and $clean.no_thread_leak)
    }
    Save-Result 'leak-cycles' $d $checks ([ordered]@{ growth = $growth; shutdown = $clean })
}

function Test-SoftwareBaseline {
    $d = New-TestDir 'software-baseline'
    Start-Mtx $d
    $eng = Start-Engine $d 'config\software-encoder.toml' @('--stream', '--record')
    Start-Sleep 60
    $pull = Get-StreamSample $d 5 'pull'
    $r = Jk record-stop; Jk stream-stop | Out-Null
    $rec = Test-Decode (Get-RecordingPath $r)
    $m = Measure-Stats $d 10
    $stop = Stop-Engine $eng
    $clean = Test-CleanShutdown $d $stop
    $checks = [ordered]@{
        recording_decodes = $rec.ok; stream_decodes = $pull.ok
        clean_shutdown = ($clean.exited -and $clean.exit_code -eq 0 -and $clean.no_thread_leak)
    }
    Save-Result 'software-baseline' $d $checks ([ordered]@{ stats = $m; recording = $rec; pull = $pull })
}

function Test-Sustained {
    $d = New-TestDir 'sustained'
    Start-Mtx $d
    $eng = Start-Engine $d 'config\baseline.toml' @('--stream', '--record')
    $pv = Start-Process $Preview -PassThru -RedirectStandardError (Join-Path $d 'preview.log') -RedirectStandardOutput (Join-Path $d 'preview.out')
    $nv = Start-Process nvidia-smi -ArgumentList '--query-gpu=timestamp,utilization.gpu,utilization.encoder,memory.used,power.draw,temperature.gpu,clocks.gr', '--format=csv', '-l', '5' `
        -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $d 'nvidia-smi.csv') -RedirectStandardError (Join-Path $d 'nvidia-smi.err')
    $end = (Get-Date).AddMinutes($SustainMin)
    while ((Get-Date) -lt $end -and -not $eng.HasExited) { Start-Sleep 30 }
    $pull = Get-StreamSample $d 10 'pull-end'
    $r = Jk record-stop; Jk stream-stop | Out-Null
    $nv | Stop-Process -Force -ErrorAction SilentlyContinue
    $pv.Refresh(); Close-JkWin $pv.MainWindowHandle
    $pvExit = $pv.WaitForExit(10000)
    # Preview lines: "<copied> fps copied, <presented> fps presented, <busy> busy, worst gap <ms> ms"
    $pvl = Select-String -Path (Join-Path $d 'preview.log') -Pattern '([\d.]+) fps copied, ([\d.]+) fps presented, (\d+) busy, worst gap ([\d.]+) ms' |
        ForEach-Object { $g = $_.Matches[0].Groups; [pscustomobject]@{ copied = [double]$g[1].Value; presented = [double]$g[2].Value; busy = [int]$g[3].Value; gap = [double]$g[4].Value } } | Select-Object -Skip 3
    $preview = [ordered]@{
        seconds = @($pvl).Count; exited_cleanly = $pvExit
        copied_fps_avg = [math]::Round(($pvl | Measure-Object copied -Average).Average, 1)
        copied_fps_min = ($pvl | Measure-Object copied -Minimum).Minimum
        presented_fps_avg = [math]::Round(($pvl | Measure-Object presented -Average).Average, 1)
        worst_present_gap_ms_p99 = Pct ($pvl | ForEach-Object gap) 99
        worst_present_gap_ms_max = ($pvl | Measure-Object gap -Maximum).Maximum
        busy_slot_skips = ($pvl | Measure-Object busy -Sum).Sum
    }
    $rec = Test-Decode (Get-RecordingPath $r)
    $m = Measure-Stats $d 10
    $stop = Stop-Engine $eng
    $clean = Test-CleanShutdown $d $stop
    $checks = [ordered]@{
        ran_full_duration      = ($m.duration_s -ge ($SustainMin * 60 - 30))
        lagged_under_0_1_pct   = ($m.render_lagged_pct -lt 0.1)
        # §1: tick jitter p99 <= 1 ms; render thread <= 2 ms per tick (wall time
        # here, an upper bound on the thread CPU time the budget names).
        jitter_p99_under_1ms   = ($m.jitter_ms_p99_10s_max -le 1.0)
        render_tick_mean_under_2ms = ($m.tick_ms_mean -le 2.0)
        # performance-budgets.md §1: compositor 1.0 + preview 0.4 ms, 0.3 ms per
        # encoder conversion, engine total p95 <= 4 ms over 10 s.
        gpu_composite_preview_under_1_4ms = ($m.gpu_composite_preview_ms_mean -le 1.4)
        # Conversion is not separately measurable on this machine (see
        # Measure-Stats), so check the whole engine: 1.0 + 0.4 + 2 x 0.3 ms.
        gpu_engine_mean_under_2ms = (($m.gpu_3d_ms_per_frame_pdh + $m.gpu_convert_ms_per_frame_pdh) -le 2.0)
        gpu_engine_p95_under_4ms = (($m.gpu_composite_preview_ms_p95_max + $m.gpu_convert_ms_per_frame_pdh) -le 4.0)
        cpu_under_1_5_cores    = ($m.cpu_cores_avg -le 1.5)
        ram_under_1_2gb        = ([double]($m.working_set_mb_first_last -split ' -> ')[1] -le 1228)
        memory_growth_under_5pct = ($m.private_mb_growth_pct -le 5)
        no_encoder_drops       = ($m.encoder_queue_drops -eq 0)
        preview_kept_up        = ($preview.copied_fps_avg -ge 55)
        preview_exited_cleanly = $pvExit
        recording_decodes      = $rec.ok
        stream_decodes         = $pull.ok
        clean_shutdown         = ($clean.exited -and $clean.exit_code -eq 0 -and $clean.no_thread_leak)
    }
    Save-Result 'sustained' $d $checks ([ordered]@{ minutes = $SustainMin; stats = $m; preview = $preview; recording = $rec; stream_pull = $pull; shutdown = $clean })
}

# Interactive: physical camera removal and reinsertion.
function Test-DeviceRemoval {
    $d = New-TestDir 'device-removal'
    $eng = Start-Engine $d 'config\baseline.toml' @('--record')
    Wait-Until { (Stats).camera.status.state -eq 'active' } 10 | Out-Null
    Write-Host 'Unplug the Insta360 Link 2C camera now (waiting up to 120 s)...'
    $t = Get-Date
    $gone = Wait-Until { (Stats).camera.status.code -eq 'CAP.CAMERA.DISCONNECTED' } 120 0.2
    $goneS = [math]::Round(((Get-Date) - $t).TotalSeconds, 1)
    $mid = Stats
    Write-Host 'Plug it back in (waiting up to 120 s)...'
    $back = Wait-Until { (Stats).camera.status.state -eq 'active' } 120
    Start-Sleep 3
    $r = Jk record-stop
    $rec = Test-Decode (Get-RecordingPath $r)
    $stop = Stop-Engine $eng
    $clean = Test-CleanShutdown $d $stop
    $checks = [ordered]@{
        disconnect_reported = $gone; recording_continued = ((Output $mid 'record').status.state -eq 'active')
        camera_recovered = $back; recording_decodes = $rec.ok
        clean_shutdown = ($clean.exited -and $clean.exit_code -eq 0 -and $clean.no_thread_leak)
    }
    Save-Result 'device-removal' $d $checks ([ordered]@{ camera_during = $mid.camera; recording = $rec })
}

# -------------------------------------------------------------------- main

$map = @{
    'devices' = { Test-Devices }; 'start-stop' = { Test-StartStop }; 'invalid-input' = { Test-InvalidInput }
    'encoder-errors' = { Test-EncoderErrors }; 'simultaneous' = { Test-Simultaneous }; 'network' = { Test-Network }
    'leak-cycles' = { Test-LeakCycles }; 'software-baseline' = { Test-SoftwareBaseline }; 'sustained' = { Test-Sustained }
    'device-removal' = { Test-DeviceRemoval }
}
Write-Host "results: $ResultsRoot"
foreach ($t in $Tests) {
    try { & $map[$t] }
    catch {
        Write-Host "[ERROR] ${t}: $_"
        $script:Summary[$t] = $false
        Get-Process jalakam-engine-poc -ErrorAction SilentlyContinue | Stop-Process -Force
    }
}
Stop-Mtx
$script:Summary | ConvertTo-Json | Out-File (Join-Path $ResultsRoot 'summary.json') -Encoding utf8
$script:Summary.GetEnumerator() | Sort-Object Name | Format-Table Name, Value -AutoSize
