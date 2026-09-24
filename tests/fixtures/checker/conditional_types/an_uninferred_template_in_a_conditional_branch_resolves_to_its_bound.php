<?php
            use function PHPStan\Testing\assertType;
            class Connection {}
            class DriverManager {
                /**
                 * @param array<string, mixed> $params
                 * @template T of Connection
                 * @return ($params is array{wrapperClass: class-string<T>} ? T : Connection)
                 */
                public static function getConnection(array $params): Connection { return new Connection(); }
            }
            /** @param array<string, mixed> $params */
            function f(array $params): void {
                assertType('Connection', DriverManager::getConnection($params));
            }
        