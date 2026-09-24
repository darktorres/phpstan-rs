<?php
            #[Attribute]
            class AllowedLegacy {}
            class Db { public function query(string $sql) {} }
            class Repo {
                #[AllowedLegacy]
                public function f(Db $db): void { $db->query('select 1'); }
            }
        