# GVR Hybrid Node · Windows Guide

Этот документ описывает, как запустить GVR Hybrid Node на Windows 10/11:

- в локальном режиме (только для себя),
- и как (по желанию) сделать свою ноду доступной из интернета, чтобы другие могли к ней подключаться.

Документ ориентирован на **обычного пользователя Windows**, без опыта Linux/VPS.

---

## 1. Требования

- Windows 10 или 11 (64‑bit).
- Свободное место на диске (от 1–2 ГБ).
- Установленный **Rust** и **Git**.

### 1.1. Установка Rust

1. Зайди на сайт: <https://www.rust-lang.org/tools/install>  
2. Скачай и запусти `rustup-init.exe`.
3. В установщике выбери вариант `1) Proceed with installation (default)`.

После установки **закрой и заново открой** PowerShell или CMD.

Проверка:

```powershell
rustc --version
cargo --version
```

Если команды выводят версии — Rust установлен.

### 1.2. Установка Git

Скачать и установить Git: <https://git-scm.com/download/win>  
В установщике можно оставлять настройки по умолчанию.

Проверка:

```powershell
git --version
```

---

## 2. Клонирование и сборка GVR Hybrid Node

Открой PowerShell и выбери папку, где будет проект, например:

```powershell
cd C:\Users\Пользователь
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node
```

Сборка в release‑режиме:

```powershell
cargo build --release
```

После этого в `target\release` появятся бинарники:

- `gvr-node.exe` — основная нода,
- `gvr-wallet.exe` — CLI‑кошелёк,
- `gvr-client.exe` — простой RPC‑клиент,
- `gvr-p2p-client.exe` — P2P‑клиент,
- `gvr-ai-keygen.exe` — генерация AI‑ключа,
- `gvr-energy-client.exe` — клиент для `/energy_proof`,
- `gvr-ai-rotate.exe` — инструмент ротации AI‑ключа.

---

## 3. Генерация AI‑ключа (один раз)

AI‑ключ используется для подписи и проверки EnergyProof.

В PowerShell:

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node

.\target\release\gvr-ai-keygen.exe
```

После этого в папке проекта появятся файлы:

- `ai_key.bin` — приватный AI‑ключ (не публиковать),
- `ai_pubkey.bin` — публичный AI‑ключ.

---

## 4. Запуск ноды (локальный режим)

### 4.1. Простой запуск ноды

В PowerShell:

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node

cargo run --release --bin gvr-node -- `
  --p2p-addr 127.0.0.1:4000 `
  --rpc-addr 127.0.0.1:8080 `
  --coinbase-addr alice `
  --ai-key-file ai_key.bin
```

Пояснения:

- `--p2p-addr 127.0.0.1:4000` — P2P доступен только локально (только эта машина).
- `--rpc-addr 127.0.0.1:8080` — RPC (и веб‑UI) доступны только на этом ПК.
- `--coinbase-addr alice` — награды за блоки и комиссии будут начисляться на адрес `alice` в state.

Нода начнёт майнить блоки и каждые 10 секунд печатать статус сети в консоль.

### 4.2. Проверка RPC и веб‑интерфейса

В другом окне PowerShell:

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node
curl http://127.0.0.1:8080/status
```

В браузере:

```text
http://127.0.0.1:8080/ui
```

Отобразится веб‑страница с текущим статусом ноды и простыми RPC‑инструментами.

---

## 5. Кошелёк и отправка транзакции

### 5.1. Создать кошелёк

Создадим кошелёк `alice` (имя кошелька = адрес в state):

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node

.\target\release\gvr-wallet.exe new --name alice
```

Вывод покажет:

- путь к приватному ключу (`wallets\alice.key`),
- адрес (`alice`),
- публичный ключ (hex).

### 5.2. Проверить баланс

```powershell
curl "http://127.0.0.1:8080/balance?addr=alice"
```

Если нода уже что‑то намайнила и coinbase стоит `alice`, баланс будет > 0.

### 5.3. Отправить транзакцию с `alice` на `bob`

Создадим тестовый `bob` только как строковый адрес (ему не нужен ключ для получения):

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node

.\target\release\gvr-wallet.exe send `
  --rpc 127.0.0.1:8080 `
  --from-wallet alice `
  --to bob `
  --amount 10 `
  --fee 1
```

Кошелёк:

- запросит `nonce` для `alice` через `/nonce?addr=alice`,
- подпишет транзакцию,
- отправит её на ноду (`/tx`).

После того как майнер включит её в блок, можно проверить:

```powershell
curl "http://127.0.0.1:8080/balance?addr=alice"
curl "http://127.0.0.1:8080/balance?addr=bob"
```

---

## 6. Отправка EnergyProof (по желанию)

Чтобы протестировать энергокомпоненту:

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node

.\target\release\gvr-energy-client.exe `
  --rpc 127.0.0.1:8080 `
  --producer_id my_station_1 `
  --sequence 1 `
  --kwh 123.45 `
  --ai_score 0.92
```

Клиент:

- сформирует `EnergyProof`,
- подпишет его `ai_key.bin`,
- проверит подпись локально,
- отправит DTO на `/energy_proof`.

В логах ноды появится сообщение об успешном приёме доказательства. На фазах Phase2/Phase3 это будет влиять на награду за блок.

---

## 7. Открыть ноду в Интернет (опционально)

Если хочешь, чтобы **к твоей ноде могли подключаться другие люди** (как к seed‑ноде), нужно:

### 7.1. Меняем адрес P2P на 0.0.0.0

Запускать ноду не с `127.0.0.1`, а с `0.0.0.0` по P2P:

```powershell
cd C:\Users\Пользователь\gvr-hybrid-node

cargo run --release --bin gvr-node -- `
  --p2p-addr 0.0.0.0:4000 `
  --rpc-addr 127.0.0.1:8080 `
  --coinbase-addr alice `
  --ai-key-file ai_key.bin
```

Так P2P‑порт будет слушать на всех интерфейсах, но RPC останется доступным только локально (безопаснее).

### 7.2. Открыть порт 4000 в Windows Firewall

Открой PowerShell **от имени администратора**:

```powershell
New-NetFirewallRule -DisplayName "GVR P2P" -Direction Inbound -LocalPort 4000 -Protocol TCP -Action Allow
```

Если хочешь, чтобы и RPC был доступен снаружи:

```powershell
New-NetFirewallRule -DisplayName "GVR RPC" -Direction Inbound -LocalPort 8080 -Protocol TCP -Action Allow
```

### 7.3. Проброс порта 4000 на роутере

1. В браузере зайди в панель управления роутера (обычно `http://192.168.0.1` или `http://192.168.1.1`).
2. Найди раздел «Port Forwarding» / «NAT» / «Virtual Server».
3. Добавь правило:

   - Внешний порт: `4000` (TCP),
   - Внутренний IP: IP твоего ПК в локальной сети (смотрим через `ipconfig`, обычно `192.168.0.X`),
   - Внутренний порт: `4000` (TCP).

(Аналогично можно пробросить 8080, если нужен внешний доступ к RPC/UI.)

### 7.4. Узнать внешний IP

В PowerShell:

```powershell
curl ifconfig.me
```

Допустим, ответ: `1.2.3.4`.

Теперь твоя нода доступна как:

- P2P: `1.2.3.4:4000`
- (опционально) RPC/UI: `1.2.3.4:8080`

Другой узел может подключиться к тебе с параметром:

```bash
--peers 1.2.3.4:4000
```

Пример запуска у другого пользователя:

```bash
gvr-node --p2p-addr 0.0.0.0:4000 --rpc-addr 127.0.0.1:8080 --coinbase-addr bob --ai-pubkey-file ai_pubkey.bin --peers 1.2.3.4:4000
```

---

## 8. Корректное завершение ноды

Чтобы нода сохранила состояние (`state.json`) и корректно остановилась:

- в окне с работающей нодой нажми `Ctrl + C`,
- дождись сообщения о сохранении снапшота.

При следующем запуске нода подхватит `state.json` и продолжит с того же состояния (высота цепи, балансы, total_supply).

---

## 9. Частые вопросы

### 9.1. Можно ли запустить ноду без моего компьютера?

Да, если запустить её **на удалённом сервере (VPS)** в интернете. Тогда:

- твой ПК не участвует,
- сервер крутит `gvr-node` 24/7.

Но VPS обычно платные. Текущий гайд показывает, как запускать ноду **бесплатно у себя дома** (за исключением затрат на интернет и электричество).

### 9.2. Почему сложность иногда большая и блоки долго майнятся?

В `constants.rs` заданы параметры сложности и целевого времени блока. Для тестовой сети на домашнем ПК можно:

- уменьшить `INITIAL_DIFFICULTY`,
- сократить `TARGET_BLOCK_TIME_SEC`.

Это уже настройки протокола; см. `PROTO.md` и комментарии в `constants.rs`.

---

Для любых вопросов и багов:

- используй GitHub Issues репозитория:  
  <https://github.com/alekseymonin1992-sys/gvr-hybrid-node/issues>