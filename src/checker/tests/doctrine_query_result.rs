//! DQL-aware Doctrine ORM result-type inference (PHASE_BACKLOG Phase 168,
//! 168b-168f) - end-to-end `assertType()` coverage through a minimal
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
            public function createQueryBuilder(string $alias): QueryBuilder {}
            public function getEntityManager(): EntityManagerInterface {}
        }
    }
"#;

fn diagnostics_with_stubs(src: &str) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for(&format!("<?php\n{STUBS}\n{src}"))
}

fn diagnostics_with_stubs_and_config(
    src: &str,
    configure: impl FnOnce(&mut crate::registry::Registry),
) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for_with(&format!("<?php\n{STUBS}\n{src}"), configure)
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

#[test]
fn get_array_result_converts_the_entity_into_an_object_shape() {
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
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->getArrayResult();
                assertType('list<object{id: int, name: string}>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_single_column_result_of_one_scalar_field_is_a_list_of_that_type() {
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
            public function allNames(): void
            {
                $names = $this->createQueryBuilder('t')
                    ->select('t.name')
                    ->getQuery()
                    ->getSingleColumnResult();
                assertType('list<string>', $names);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_single_scalar_result_of_one_scalar_field_is_that_fields_own_type() {
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
                    ->getSingleScalarResult();
                assertType('string', $name);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_scalar_result_flattens_the_entity_under_alias_prefixed_keys() {
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
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->getScalarResult();
                assertType('list<array{t_id: int, t_name: string}>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn create_query_with_a_literal_dql_string_is_a_list_of_the_entity() {
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
                $rows = $em->createQuery('SELECT t FROM App\TagEntity t')->getResult();
                assertType('list<App\TagEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn create_query_with_a_non_literal_dql_string_falls_back_to_todays_shape_only_list() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\EntityManagerInterface;

        class TagService
        {
            public function all(EntityManagerInterface $em, string $dql): void
            {
                $rows = $em->createQuery($dql)->getResult();
                assertType('list', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_declared_custom_dql_function_resolves_to_its_own_config_category() {
    let d = diagnostics_with_stubs_and_config(
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
            public function months(): void
            {
                $rows = $this->createQueryBuilder('t')
                    ->select('t.id', 'DATE_FORMAT_YEAR_MONTH(t.name) AS ym')
                    ->getQuery()
                    ->getResult();
                assertType('list<array{0: int, ym: string}>', $rows);
            }
        }
        "#,
        |r| {
            r.doctrine_custom_functions.insert(
                "date_format_year_month".to_string(),
                crate::doctrine_functions::DoctrineFunctionKind::String,
            );
        },
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
