#!/bin/sh
# Starts the servers of one worker inside its sandbox, then runs the worker's command in their place.
# usage: servers.sh ROOT DIR COMMAND...   (see servers.php for the layout of ROOT and DIR)
ROOT="$1"
DIR="$2"
shift 2

# The sockets and pid files are in the worker's home, the one directory of its own that is not read-only.
RUN="$HOME/run"
mkdir -p "$RUN"

# The PostgreSQL programs are in the directory of their version, which is not on the PATH.
for d in /usr/lib/postgresql/*/bin; do
  PATH="$d:$PATH"
done

# Redis, on a Unix socket in the run directory.
redis-server --port 0 --unixsocket "$RUN/redis.sock" --unixsocketperm 777 --daemonize yes --dir "$RUN" --save '' --appendonly no > /dev/null 2>&1

# A Redis master on loopback, and a Sentinel that watches it: the sandbox's network namespace has its own loopback.
redis-server --port 6379 --bind 127.0.0.1 --daemonize yes --dir "$RUN" --save '' --appendonly no > /dev/null 2>&1
printf 'port 26379\nbind 127.0.0.1\ndir %s\nsentinel monitor mymaster 127.0.0.1 6379 1\nsentinel down-after-milliseconds mymaster 1000\n' "$RUN" > "$RUN/sentinel.conf"
redis-server "$RUN/sentinel.conf" --sentinel --daemonize yes > /dev/null 2>&1

# PostgreSQL, initialised in the worker's data directory and listening on a Unix socket only.
initdb -D "$DIR/pg" -U postgres --auth=trust > "$DIR/pg-init.log" 2>&1
pg_ctl -D "$DIR/pg" -o "-k $RUN -c listen_addresses=" -l "$DIR/pg.log" -w start > /dev/null 2>&1

# MySQL, from the copy of the template in the data directory, on a Unix socket only.
mysqld --no-defaults --datadir="$DIR/mysql" --socket="$RUN/mysql.sock" --skip-networking --mysqlx=OFF \
  --pid-file="$RUN/mysql.pid" --tmpdir="$RUN" --log-error="$DIR/mysql.err" --user="$(id -un)" > /dev/null 2>&1 &

# FTP, on loopback: the sandbox's own network namespace has it.
"$ROOT/ftp/bin/python" "$ROOT/ftp-server.py" > "$DIR/ftp.log" 2>&1 &

# The servers listen once their sockets exist; the worker waits for them rather than for a fixed time.
i=0
while [ ! -S "$RUN/mysql.sock" ] && [ $i -lt 60 ]; do
  sleep 0.5
  i=$((i + 1))
done

exec "$@"
