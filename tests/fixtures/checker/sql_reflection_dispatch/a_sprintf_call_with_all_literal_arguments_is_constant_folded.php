<?php

    namespace Doctrine\DBAL {
        class QueryCacheProfile {}
        class Result {
            public function fetchAssociative(): array|false {}
            public function fetchAllAssociative(): array {}
            public function fetchOne(): mixed {}
            public function fetchFirstColumn(): array {}
            public function fetchNumeric(): array|false {}
            public function fetchAllNumeric(): array {}
            public function fetchAllKeyValue(): array {}
            public function fetchAllAssociativeIndexed(): array {}
            public function iterateAssociative(): \Traversable {}
            public function iterateNumeric(): \Traversable {}
            public function iterateKeyValue(): \Traversable {}
            public function iterateAssociativeIndexed(): \Traversable {}
            public function iterateColumn(): \Traversable {}
            public function rowCount(): int|string {}
            public function columnCount(): int {}
        }
        class Connection {
            public function fetchAssociative(string $query, array $params = [], array $types = []): array|false {}
            public function fetchAllAssociative(string $query, array $params = [], array $types = []): array {}
            public function fetchOne(string $query, array $params = [], array $types = []): mixed {}
            public function fetchFirstColumn(string $query, array $params = [], array $types = []): array {}
            public function fetchNumeric(string $query, array $params = [], array $types = []): array|false {}
            public function fetchAllNumeric(string $query, array $params = [], array $types = []): array {}
            public function fetchAllKeyValue(string $query, array $params = [], array $types = []): array {}
            public function fetchAllAssociativeIndexed(string $query, array $params = [], array $types = []): array {}
            public function iterateAssociative(string $query, array $params = [], array $types = []): \Traversable {}
            public function iterateNumeric(string $query, array $params = [], array $types = []): \Traversable {}
            public function iterateKeyValue(string $query, array $params = [], array $types = []): \Traversable {}
            public function iterateAssociativeIndexed(string $query, array $params = [], array $types = []): \Traversable {}
            public function iterateColumn(string $query, array $params = [], array $types = []): \Traversable {}
            public function executeQuery(string $sql, array $params = [], array $types = []): Result {}
            public function executeStatement(string $sql, array $params = [], array $types = []): int|string {}
            public function executeCacheQuery(string $sql, array $params, array $types, QueryCacheProfile $qcp): Result {}
            public function createQueryBuilder(): Query\QueryBuilder {}
        }
    }
    namespace Doctrine\DBAL\Query {
        class QueryBuilder {
            public function select(string ...$select): static {}
            public function addSelect(string ...$select): static {}
            public function from(string $table, ?string $alias = null): static {}
            public function where(string $predicate): static {}
            public function fetchAssociative(): array|false {}
            public function fetchAllAssociative(): array {}
            public function fetchOne(): mixed {}
            public function fetchFirstColumn(): array {}
            public function executeQuery(): \Doctrine\DBAL\Result {}
            public function executeStatement(): int|string {}
        }
    }


        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $row = $c->fetchAssociative(sprintf('SELECT id, name FROM %s', 'categories'));
            assertType('array{id: int, name: string}|false', $row);
        }
        