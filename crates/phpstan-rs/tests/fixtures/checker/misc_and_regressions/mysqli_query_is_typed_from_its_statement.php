<?php
            use function PHPStan\Testing\assertType;
            function f(mysqli $db, string $sql): void {
                assertType('true', $db->query('SET NAMES utf8mb4'));
                assertType('mysqli_result|true', $db->query($sql));
                assertType('true', mysqli_query($db, 'DELETE FROM t'));
            }
        