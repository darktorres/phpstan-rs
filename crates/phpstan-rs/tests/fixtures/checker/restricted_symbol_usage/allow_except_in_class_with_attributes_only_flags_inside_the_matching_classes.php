<?php
            class Db { public function query(string $sql) {} }
            #[Special]
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
            class OtherRepo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
        