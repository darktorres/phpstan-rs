<?php
            class Db { public function query(string $sql) {} }
            function f(Db $db): void { $db->query('select 1'); }
        