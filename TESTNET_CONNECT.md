# GVR Hybrid Testnet · How to Connect to Aleksey's Seed Node

Этот документ описывает, как подключиться к публичной seed‑ноде GVR, запущенной на компьютере автора, и синхронизироваться с сетью.

---

## 1. Адрес seed‑ноды

- **P2P:** `95.191.235.94:4000`

Нода автора запущена с параметрами:

```text
--p2p-addr 0.0.0.0:4000
--rpc-addr 127.0.0.1:8080
--coinbase-addr alekseymonin1992
--ai-key-file ai_key.bin
```

RPC (`8080`) доступен только локально на машине автора. Для подключения к сети используется именно P2P‑порт `4000`.

---

## 2. Подключение с Windows (PowerShell)

### 2.1. Требования

- Установлен Rust и Cargo (см. `WINDOWS_GUIDE.md`).
- Установлен Git.

Проверка:

```powershell
rustc --version
cargo --version
git --version
```

### 2.2. Клонировать репозиторий и собрать

```powershell
cd C:\Users\Пользователь
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node

cargo build --release
```

### 2.3. Запуск вашей ноды с подключением к seed

В PowerShell:

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node

.\target\release\gvr-node.exe --p2p-addr 0.0.0.0:4000 --rpc-addr 127.0.0.1:8080 --coinbase-addr alice --peers 95.191.235.94:4000
```

Пояснения:

- `--peers 95.191.235.94:4000` — адрес seed‑ноды автора.
- `--p2p-addr 0.0.0.0:4000` — ваша нода тоже слушает на 4000 (можно поставить `127.0.0.1:4000`, если не хотите принимать внешние подключения).
- `--rpc-addr 127.0.0.1:8080` — RPC и веб‑UI только локально.
- `--coinbase-addr alice` — награды за блоки и комиссии будут начисляться на адрес `alice` в вашей локальной цепи.

### 2.4. Синхронизация с seed‑нодой

В другом окне PowerShell:

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node

curl "http://127.0.0.1:8080/sync?peer=95.191.235.94:4000"
```

Нода начнёт запрашивать блоки у `95.191.235.94:4000` и подтягивать основную цепь.

### 2.5. Проверка статуса

```powershell
curl http://127.0.0.1:8080/status
```

Если всё ок:

- `height` > 0 и растёт,
- `total_supply` > 0 и растёт,
- фаза (`phase`) и сложность (`difficulty`) совпадают (примерно) с сетью.

Веб‑интерфейс:

```text
http://127.0.0.1:8080/ui
```

---

## 3. Подключение с Linux

### 3.1. Установка зависимостей и Rust

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev curl git

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### 3.2. Клонировать и собрать

```bash
cd $HOME
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node

cargo build --release
```

### 3.3. Запуск ноды с подключением к seed

```bash
cd $HOME/gvr-hybrid-node

target/release/gvr-node \
  --p2p_addr 0.0.0.0:4000 \
  --rpc_addr 127.0.0.1:8080 \
  --coinbase_addr alice \
  --peers 95.191.235.94:4000
```

### 3.4. Синхронизация и проверка

```bash
curl "http://127.0.0.1:8080/sync?peer=95.191.235.94:4000"
curl http://127.0.0.1:8080/status
```

Веб‑UI:

```text
http://127.0.0.1:8080/ui
```

---

## 4. Мини‑чек‑лист: вы в сети?

- [ ] Нода запущена с `--peers 95.191.235.94:4000`.
- [ ] Вызван `/sync?peer=95.191.235.94:4000`.
- [ ] `GET /status` на вашей ноде возвращает:
  - высоту (`height`) больше нуля и растущую,
  - `total_supply` > 0.
- [ ] В веб‑UI (`/ui`) виден рост числа блоков и суммарной эмиссии.

Если есть проблемы с подключением:

1. Проверьте, что `95.191.235.94:4000` доступен из вашей сети (иногда провайдеры/файрволлы блокируют исходящие порты).
2. Смотрите логи вашей ноды — сообщения из P2P‑подсистемы (`p2p.rs`) подскажут, удалось ли установить соединение.
3. При необходимости задайте вопрос через GitHub Issues в репозитории.
```

Если захочешь, можем ещё добавить в README небольшой блок:

```md
## Public testnet seed

Current public seed node (run by the author):

- `95.191.235.94:4000`

See [TESTNET_CONNECT.md](TESTNET_CONNECT.md) for connection 