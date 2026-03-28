# GVR Hybrid Testnet · Подключение к seed‑ноде

Этот документ объясняет, как подключиться к публичной seed‑ноде GVR, запущенной на компьютере автора, и синхронизироваться с сетью. Поддерживаются Windows и Linux.

---

## 1. Адрес seed‑ноды

Seed‑нода автора (Windows‑узел дома):

- **P2P:** `95.191.235.94:4000`

Нода автора запущена с параметрами:

```text
--p2p-addr 0.0.0.0:4000
--rpc-addr 127.0.0.1:8080
--coinbase-addr alekseymonin1992
--ai-key-file ai_key.bin
RPC (
8080
) доступен только локально на машине автора. Для подключения к сети используется P2P‑порт
4000
.

## 2. Подключение с Windows (простая инструкция)
Ниже шаги для Windows 10/11. Команды можно вводить в:

«Windows PowerShell» (Пуск → ввести
PowerShell
),
или в «Командная строка» (Пуск → ввести
cmd
).
Где в примерах написано
ИмяПользователя
, подставь свой логин Windows, например:
C:\Users\Ivan
.

## 2.1. Установить Rust и Git
Rust
Зайди на https://www.rust-lang.org/tools/install
Скачай
rustup-init.exe
и установи (вариант по умолчанию —
1) Proceed with installation
).

Git
Зайди на https://git-scm.com/download/win
Скачай установщик и установи с настройками по умолчанию.

Проверка (в новом окне PowerShell/cmd):

rustc --version
cargo --version
git --version
Если команды выводят версии — всё хорошо.

## 2.2. Скачать и собрать GVR Hybrid Node
cd C:\Users\ИмяПользователя
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node
cargo build --release
Эта команда:

скачает исходники,
соберёт исполняемые файлы в
target\release
.
## 3. Запустить ноду и подключиться к seed
Теперь запустим твою ноду, чтобы она подключилась к узлу автора
95.191.235.94:4000
.

cd C:\Users\ИмяПользователя\gvr-hybrid-node

.\target\release\gvr-node.exe --p2p-addr 127.0.0.1:4000 --rpc-addr 127.0.0.1:8080 --coinbase-addr myminer --peers 95.191.235.94:4000
Что здесь важно:

--p2p-addr 127.0.0.1:4000
— твоя нода слушает P2P на локальном порту 4000.
--rpc-addr 127.0.0.1:8080
— интерфейс управления и веб‑страница доступны только с этого компьютера.
--coinbase-addr myminer
— имя твоего адреса в сети.
Это просто строка. Вместо
myminer
можно написать любое своё имя:
ivan
,
test1
,
mywallet
и т.д.
В будущем все награды за блоки и комиссии будут начисляться на этот адрес.
--peers 95.191.235.94:4000
— подключение к seed‑ноде автора.
Это окно оставляем открытым — там будет идти лог и майнинг.

## 4. Синхронизация с сетью
Открой второе окно PowerShell/cmd:

cd C:\Users\ИмяПользователя\gvr-hybrid-node

curl "http://127.0.0.1:8080/sync?peer=95.191.235.94:4000"
curl http://127.0.0.1:8080/status
Если всё ок, в ответ на
/status
ты увидишь JSON, где:

height
больше 0 и со временем растёт,
total_supply
больше 0 и растёт,
phase
и
difficulty
примерно соответствуют сети автора.
## 5. Веб‑интерфейс
Открой браузер и перейди по адресу:

http://127.0.0.1:8080/ui
Ты увидишь веб‑страницу со статусом ноды и простыми RPC‑инструментами (балансы, nonce, mempool).

## 6. Свой кошелёк и майнинг на него (Windows)
По умолчанию мы указали
--coinbase-addr myminer
. Это значит, что:

все блоковые награды и комиссии будут начисляться на адрес
myminer
в твоём состоянии;
чтобы управлять этим балансом, создадим локальный кошелёк.
## 6.1. Создать кошелёк с СВОИМ именем
Имя кошелька = имя адреса в сети.
Если ты уже выбрал
myminer
в
--coinbase-addr
, имеет смысл использовать такое же имя.

В новом окне PowerShell/cmd:

cd C:\Users\ИмяПользователя\gvr-hybrid-node

.\target\release\gvr-wallet.exe new --name myminer
Эта команда:

создаст файл приватного ключа
wallets\myminer.key
локально (он не попадает в интернет),
выведет на экран:
адрес (строка
myminer
),
публичный ключ (hex).
Проверить кошелёк:

.\target\release\gvr-wallet.exe show --name myminer
## 6.2. Проверить, что на адрес капают награды
Через некоторое время работы ноды (когда найдутся блоки) можно проверить баланс:

curl "http://127.0.0.1:8080/balance?addr=myminer"
Если майнинг идёт, баланс
myminer
будет расти.

## 6.3. Отправить монеты с кошелька на другой адрес
Допустим, ты хочешь перевести 10 GVR с
myminer
на адрес
friend1
:

cd C:\Users\ИмяПользователя\gvr-hybrid-node

.\target\release\gvr-wallet.exe send `
  --rpc 127.0.0.1:8080 `
  --from-wallet myminer `
  --to friend1 `
  --amount 10 `
  --fee 1
Параметры:

from-wallet myminer
— твой кошелёк‑отправитель,
to friend1
— строка‑адрес получателя (имя можно придумать любое, получателю не нужен кошелёк, чтобы просто получить средства),
amount 10
— сумма,
fee 1
— комиссия за транзакцию.
После того как транзакция попадёт в блок, можно проверить:

curl "http://127.0.0.1:8080/balance?addr=myminer"
curl "http://127.0.0.1:8080/balance?addr=friend1"
## 7. Подключение с Linux (кратко)
Для Linux (Ubuntu/Debian) шаги похожи.

## 7.1. Установка зависимостей и Rust
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev curl git

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
## 7.2. Клонировать репозиторий и собрать
cd $HOME
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node

cargo build --release
## 7.3. Запуск ноды и подключение к seed
cd $HOME/gvr-hybrid-node

target/release/gvr-node \
  --p2p_addr 0.0.0.0:4000 \
  --rpc_addr 127.0.0.1:8080 \
  --coinbase_addr myminer \
  --peers 95.191.235.94:4000
## 7.4. Синхронизация и UI
curl "http://127.0.0.1:8080/sync?peer=95.191.235