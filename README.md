# pico-usb-cdc-ncm-rust

Raspberry Pi Pico (RP2040) + Rust/Embassy で、USB CDC-NCM（USB Ethernet）を実装するサンプルです。

このリポジトリでは、以下を確認できます。

- Pico が **USB CDC-NCM のネットワークアダプタ**として認識される
- Pico 側の IPv4 を **`172.31.1.1/24`** に固定する
- Pico 側で **DHCPv4 サーバー**を動かし、ホスト PC に **`172.31.1.2`** を払い出す
- Pico からホスト PC（`172.31.1.2:8080`）へ **HTTP GET** を送信し、レスポンスをログ出力する
- ホスト PC 側で **Rust 製 HTTP サーバー**を `0.0.0.0:8080` で待ち受ける

## リポジトリ構成

- `firmware/`: Pico 向け no_std ファームウェア（Embassy）
- `host-http/`: ホスト PC 側 Rust HTTP サーバー

## 前提

- Rust toolchain
- RP2040 向けターゲット追加:
  - `rustup target add thumbv6m-none-eabi`
- 書き込み方法（どちらか）:
  - `probe-rs`（`firmware/.cargo/config.toml` の runner を使用）
  - UF2（`elf2uf2-rs` 等）
- Raspberry Pi Pico (RP2040)

`probe-rs` を使う場合は、事前に `probe-rs-tools` をインストールしてください。

```bash
cargo install probe-rs-tools
```

## 1) ホスト PC 側 HTTP サーバー起動

```bash
cargo run -p pico-usb-cdc-ncm-host-http
```

起動ログ例:

```text
[host-http] listening on 0.0.0.0:8080
```

Pico からリクエストが来ると、以下のように表示されます:

```text
[host-http] peer=Some(172.31.1.1:xxxxx) request="GET / HTTP/1.1"
```

## 2) Pico ファームウェアのビルド / 書き込み

リポジトリルートから実行します。

```bash
cargo build -p pico-usb-cdc-ncm-firmware --release --target thumbv6m-none-eabi --bin cdc_ncm
```

`firmware/.cargo/config.toml` の runner（`probe-rs`）を使う場合:

```bash
cargo run -p pico-usb-cdc-ncm-firmware --target thumbv6m-none-eabi --bin cdc_ncm
```

## 3) Windows 側の確認手順

1. Pico を USB で接続します。
2. デバイスマネージャーでネットワークアダプタが増えていることを確認します。
3. `ipconfig` を実行し、該当アダプタに `172.31.1.2` が設定されていることを確認します。
4. `host-http` サーバーを起動します（上記の `cargo run ...`）。
5. ファームウェアのログで HTTP レスポンスが出力されることを確認します。

## firmware 側サンプル（`src/bin`）

- `cdc_ncm`: CDC-NCM として認識 + Pico 側固定 IP（`172.31.1.1/24`）
- `dhcp_server`: `cdc_ncm` + DHCP でホスト IP `172.31.1.2` を OFFER/ACK
- `http_get`: `dhcp_server` + `172.31.1.2:8080` へ HTTP GET

実行例:

```bash
cargo run -p pico-usb-cdc-ncm-firmware --target thumbv6m-none-eabi --bin dhcp_server
```

```bash
cargo run -p pico-usb-cdc-ncm-firmware --target thumbv6m-none-eabi --bin http_get
```

## ファームウェアの動作メモ

- Pico 側固定 IP: `172.31.1.1`
- DHCP 払い出し先: `172.31.1.2`
- DHCP サーバー: Pico 側 UDP 67
- HTTP クライアント送信先: `172.31.1.2:8080`

## 注意点

- Windows の CDC-NCM は **Windows 10 Version 1903 以降**または **Windows 11** が必要です。
- 本サンプルはホスト IP を `172.31.1.2` に固定しています。
  - 同一のデバイスを複数同時接続すると IP が衝突し、通信できません。
  - 複数台使う場合はサブネットや IP をずらしてください。

