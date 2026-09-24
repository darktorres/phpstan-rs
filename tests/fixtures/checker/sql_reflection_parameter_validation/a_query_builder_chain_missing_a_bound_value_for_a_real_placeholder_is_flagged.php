<?php

    namespace Doctrine\DBAL {
        enum ArrayParameterType {
            case INTEGER;
            case STRING;
        }
        enum ParameterType {
            case INTEGER;
            case STRING;
        }
        class Connection {
            public function fetchAssociative(string $query, array $params = [], array $types = []): array|false {}
            public function executeQuery(string $sql, array $params = [], array $types = []): mixed {}
            public function executeStatement(string $sql, array $params = [], array $types = []): int|string {}
            public function createQueryBuilder(): Query\QueryBuilder {}
        }
    }
    namespace Doctrine\DBAL\Types {
        class Types {
            const INTEGER = 'integer';
            const SMALLINT = 'smallint';
            const BIGINT = 'bigint';
            const STRING = 'string';
        }
    }
    namespace Doctrine\DBAL\Query {
        class QueryBuilder {
            public function select(string ...$select): static {}
            public function from(string $table, ?string $alias = null): static {}
            public function where(string $predicate): static {}
            public function setParameter(int|string $key, mixed $value, mixed $type = null): static {}
            public function setParameters(array $params, array $types = []): static {}
            public function fetchAssociative(): array|false {}
        }
    }


        namespace App;

        use Doctrine\DBAL\Connection;

        function f(Connection $c): void {
            $c->createQueryBuilder()
                ->select('id')
                ->from('categories')
                ->where('status = :status')
                ->fetchAssociative();
        }
        