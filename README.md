# mt5-markout

Real-time MetaTrader 5 market data visualizer and streaming bridge using [`mt5-bridge`](../mt5-bridge) and [`markout`](../markout).

## Features

- **Live Streaming to Web**: Feeds real-time ticks and forming/closing OHLCV bars into [`markout::EventBus`](../markout/src/event_bus.rs).
- **Embedded Web Charting Interface**: Opens an interactive trading terminal and charting dashboard in your browser.
- **Wine IPC Direct Integration**: Connects over Windows Named Pipe (`\\.\pipe\mt5bridge`) to MetaTrader 5 running in Wine with token authentication (secret `test`).
- **Preloaded Historical Bars**: Automatically queries the last $N$ historical bars so the chart renders complete history immediately on connection.

## Prerequisites

1. **MetaTrader 5** running inside Wine (`/home/bariss/.wine/drive_c/Program Files/MetaTrader 5/`).
2. **`mt5_bridge.ex5`** Expert Advisor attached to any chart with `InpPipeSecret` set to `test`.
3. **Rust target `x86_64-pc-windows-gnu`**:
   ```bash
   rustup target add x86_64-pc-windows-gnu
   ```

## Quick Start

Run using the runner script:

```bash
cd /mnt/data6/MyFiles/Projects/mt5-markout
./run.sh --symbol EURUSD --tf M1 --port 8080
```

Or run manually:

```bash
cargo build --target x86_64-pc-windows-gnu
MT5_PIPE_SECRET=test wine target/x86_64-pc-windows-gnu/debug/mt5-markout.exe --symbol EURUSD --tf M1 --port 8080
```

Then open your browser at **[http://127.0.0.1:8080](http://127.0.0.1:8080)**.

## CLI Options

| Option | Env Var | Default | Description |
|---|---|---|---|
| `-s, --symbol <SYM>` | | `EURUSD` | Symbol to stream (e.g. `EURUSD`, `GBPUSD`, `BTCUSD`) |
| `-t, --tf <TF>` | | `M1` | Chart timeframe (`M1`, `M5`, `M15`, `H1`, `D1`, etc.) |
| `--history-bars <N>` | | `300` | Number of historical bars to preload into chart |
| `--secret <SECRET>` | `MT5_PIPE_SECRET` | `test` | EA pipe secret token |
| `-p, --port <PORT>` | | `8080` | Web server port to listen on |
| `--host <HOST>` | | `127.0.0.1` | Web server host address |
| `--dll-path <PATH>` | `MT5_DLL_PATH` | auto | Explicit path to `mt5_bridge.dll` |
| `--poll-interval-ms <MS>` | | `500` | Bar refresh polling interval in ms |
| `--no-ticks` | | `false` | Disable tick streaming (only stream bars) |
| `--no-bars` | | `false` | Disable bar streaming (only stream ticks) |
