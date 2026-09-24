<?php

    namespace Doctrine\ORM\Mapping {
        #[\Attribute]
        class Entity {
            public function __construct(public ?string $repositoryClass = null) {}
        }
        #[\Attribute]
        class Id {}
        #[\Attribute]
        class Column {}
        #[\Attribute]
        class ManyToOne {}
        #[\Attribute]
        class OneToMany {
            public function __construct(public string $mappedBy, public ?string $targetEntity = null) {}
        }
        #[\Attribute]
        class DiscriminatorMap {
            /** @param array<string, class-string> $value */
            public function __construct(public array $value) {}
        }
    }
    namespace Doctrine\ORM {
        class QueryBuilder {
            public function select(string ...$select): static {}
            public function addSelect(string ...$select): static {}
            public function from(string $from, string $alias, ?string $indexBy = null): static {}
            public function indexBy(string $alias, string $indexBy): static {}
            public function join(string $join, string $alias, ?string $conditionType = null, ?string $condition = null, ?string $indexBy = null): static {}
            public function innerJoin(string $join, string $alias, ?string $conditionType = null, ?string $condition = null, ?string $indexBy = null): static {}
            public function leftJoin(string $join, string $alias, ?string $conditionType = null, ?string $condition = null, ?string $indexBy = null): static {}
            public function where(string $where): static {}
            public function andWhere(string $where): static {}
            public function groupBy(string $groupBy): static {}
            public function addGroupBy(string $groupBy): static {}
            public function delete(?string $delete = null, ?string $alias = null): static {}
            public function update(?string $update = null, ?string $alias = null): static {}
            public function getQuery(): Query {}
        }
        abstract class AbstractQuery {
            const HYDRATE_OBJECT = 1;
            const HYDRATE_ARRAY = 2;
            const HYDRATE_SCALAR = 3;
            const HYDRATE_SINGLE_SCALAR = 4;
            const HYDRATE_SIMPLEOBJECT = 5;
            const HYDRATE_SCALAR_COLUMN = 6;
            public function getResult($hydrationMode = self::HYDRATE_OBJECT): mixed {}
            public function execute($parameters = null, $hydrationMode = null): mixed {}
            public function toIterable($parameters = [], $hydrationMode = null): iterable {}
            public function getOneOrNullResult($hydrationMode = null): mixed {}
            public function getSingleResult($hydrationMode = null): mixed {}
            public function getArrayResult(): array {}
            public function getSingleColumnResult(): array {}
            public function getSingleScalarResult(): mixed {}
            public function getScalarResult(): array {}
        }
        class Query extends AbstractQuery {}
        interface EntityManagerInterface {
            public function createQueryBuilder(): QueryBuilder;
            public function createQuery(string $dql): Query;
        }
        class EntityRepository {
            public function createQueryBuilder(string $alias, ?string $indexBy = null): QueryBuilder {}
            public function getEntityManager(): EntityManagerInterface {}
        }
    }


        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;
        use Doctrine\ORM\Query;

        #[ORM\Entity]
        class NoteEntity
        {
            #[ORM\Id]
            public int $id;

            public function __construct(
                #[ORM\Column]
                public string $body,
            ) {}
        }

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;

            public function __construct(
                #[ORM\Column]
                public string $name,
            ) {}
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $name = $this->createQueryBuilder('t')->select('t.name')->getQuery()
                    ->getOneOrNullResult(Query::HYDRATE_SINGLE_SCALAR);
                assertType('string|null', $name);

                $range = $this->createQueryBuilder('t')->select('MIN(t.id) AS lo', 'MAX(t.id) AS hi')
                    ->getQuery()->getSingleResult();
                assertType('array{lo: int|null, hi: int|null}', $range);

                $grouped = $this->createQueryBuilder('t')->select('t.name', 'MAX(t.id) AS hi')
                    ->groupBy('t.name')->getQuery()->getResult();
                assertType('list<array{name: string, hi: int}>', $grouped);

                $joined = $this->createQueryBuilder('t')->select('t.id', 'n.body')
                    ->leftJoin(NoteEntity::class, 'n', 'WITH', 'n.id = t.id')
                    ->getQuery()->getResult();
                assertType('list<array{id: int, body: string|null}>', $joined);

                $indexed = $this->createQueryBuilder('t', 't.id')->getQuery()->getResult();
                assertType('array<int, App\TagEntity>', $indexed);
            }
        }
        