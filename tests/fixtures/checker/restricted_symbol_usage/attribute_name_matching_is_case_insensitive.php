<?php
            class Db { public function query(string $sql) {} }
            class Repo {
                #[allowedlegacy]
                public function f(Db $db): void { $db->query('select 1'); }
            }
        