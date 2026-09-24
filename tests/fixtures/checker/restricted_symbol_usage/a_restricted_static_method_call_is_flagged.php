<?php
            class Db { public static function query(string $sql) {} }
            function f(): void { Db::query('select 1'); }
        