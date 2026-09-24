<?php
            class Db { public function query(string $sql) {} }
            #[AllowedLegacy]
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
        