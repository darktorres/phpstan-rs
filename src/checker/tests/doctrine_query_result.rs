//! DQL-aware Doctrine ORM result-type inference (PHASE_BACKLOG Phase 168,
//! 168b-168e) - end-to-end `assertType()` coverage through a minimal
//! Doctrine class hierarchy declared directly in each fixture (this
//! project vendors no real `doctrine-orm`/`doctrine-doctrine` stubs).

use super::helpers::*;

const STUBS: &str = r#"
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
    }
    namespace Doctrine\ORM {
        class QueryBuilder {
            public function select(string ...$select): static {}
            public function addSelect(string ...$select): static {}
            public function from(string $from, string $alias): static {}
            public function join(string $join, string $alias): static {}
            public function innerJoin(string $join, string $alias): static {}
            public function leftJoin(string $join, string $alias): static {}
            public function where(string $where): static {}
            public function andWhere(string $where): static {}
            public function groupBy(string $groupBy): static {}
            public function getQuery(): Query {}
        }
        abstract class AbstractQuery {
            const HYDRATE_OBJECT = 1;
            const HYDRATE_ARRAY = 2;
            public function getResult(): mixed {}
            public function execute($parameters = null, $hydrationMode = null): mixed {}
            public function toIterable($parameters = [], $hydrationMode = null): iterable {}
            public function getOneOrNullResult($hydrationMode = null): mixed {}
            public function getSingleResult($hydrationMode = null): mixed {}
        }
        class Query extends AbstractQuery {}
        interface EntityManagerInterface {
            public function createQueryBuilder(): QueryBuilder;
        }
        class EntityRepository {
            public function createQueryBuilder(string $alias): QueryBuilder {}
            public function getEntityManager(): EntityManagerInterface {}
        }
    }
"#;

fn diagnostics_with_stubs(src: &str) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for(&format!("<?php\n{STUBS}\n{src}"))
}

#[test]
fn get_result_via_the_explicit_entity_manager_form_is_a_list_of_the_entity() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityManagerInterface;

        #[ORM\Entity]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagService
        {
            public function all(EntityManagerInterface $em): void
            {
                $rows = $em->createQueryBuilder()
                    ->select('t')
                    ->from(TagEntity::class, 't')
                    ->getQuery()
                    ->getResult();
                assertType('list<App\TagEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_result_via_the_repository_shorthand_is_a_list_of_the_entity() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->getResult();
                assertType('list<App\TagEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn one_or_null_result_of_an_entity_plus_aggregate_select_is_the_mixed_shape_or_null() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function countAll(): void
            {
                $row = $this->createQueryBuilder('t')
                    ->select('t', 'COUNT(t.id) AS cnt')
                    ->getQuery()
                    ->getOneOrNullResult();
                assertType('array{0: App\TagEntity, cnt: int<0, max>}|null', $row);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_single_result_of_a_single_scalar_field_select_is_still_row_shaped() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

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
            public function oneName(): void
            {
                $name = $this->createQueryBuilder('t')
                    ->select('t.name')
                    ->getQuery()
                    ->getSingleResult();
                assertType('array{0: string}', $name);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bare_execute_behaves_identically_to_get_result() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->execute();
                assertType('list<App\TagEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn execute_with_an_explicit_non_object_hydration_mode_falls_back_to_mixed() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;
        use Doctrine\ORM\AbstractQuery;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->execute(null, AbstractQuery::HYDRATE_ARRAY);
                assertType('mixed', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_undecidable_chain_falls_back_to_todays_shape_only_list_for_get_result() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\EntityManagerInterface;

        class TagService
        {
            public function all(EntityManagerInterface $em, string $dql): void
            {
                $rows = $em->createQueryBuilder()->select($dql)->getQuery()->getResult();
                assertType('list', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
