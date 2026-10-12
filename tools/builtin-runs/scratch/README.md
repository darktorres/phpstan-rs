# Connected facts from scratch servers

The recorder runs without a server, so a builtin that only does user code once connected
(Redis, MySQL, PostgreSQL) is recorded as reached and nothing more. These scripts record the
connected facts instead, against one-off servers on spare ports, stopped afterwards. They are
not part of the recorder and never run on the host's own services (6379, 3306, 5432).

- `redis-connected.php`: each Redis method, under each serializer, with an object at each
  parameter. Needs Redis on 127.0.0.1:16379 (Redis 8.0.5 was used).
- `merge-redis.php`: adds the facts to a copy of the table as method rows, the union over
  serializers. It reads the table as bytes sorted with SORT_STRING, as the recorder does.

Provenance: a regenerated table must record these inputs in its sidecar (the script hashes and
the server version), so a reader can tell a connected fact from a recorded one.

- `sql-connected.php`: mysqli methods on a live MySQL 8.4 scratch server (127.0.0.1:13306) and
  pg_* functions on a live PostgreSQL 18 scratch server (127.0.0.1:15432), with an object at each
  parameter. The skip list excludes reconnects, closes, host file I/O and blocking calls.
- `scope-facts.php`: forward_static_call and forward_static_call_array, called from a method of a
  fixture class, so the class scope exists. The fixture class is part of this experiment only.
- `merge-sql.php`: adds the facts of the two scripts above to a table (method rows, calls rows).

Not covered: Memcached and FTP have no server here; their facts stay untrusted until one is available.
