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
