<?php
            use function PHPStan\Testing\assertType;
            function test(array $post): void {
                $a = isset($post['x']);
                assertType('bool', $a);
                $b = empty($post['x']);
                assertType('bool', $b);
            }
        