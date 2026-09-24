<?php
            class Db { public function query(string $sql) {} }
            class Repo {
                public function f(Db $db): void { $db->query('select 1'); }
                #[Special]
                private function checkThis(): void {}
            }
            class OtherRepo {
                public function f(Db $db): void { $db->query('select 1'); }
            }
        