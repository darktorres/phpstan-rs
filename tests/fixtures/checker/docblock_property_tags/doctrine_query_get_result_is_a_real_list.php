<?php
            namespace Doctrine\ORM;
            abstract class AbstractQuery {
                public function getResult(): mixed { return []; }
            }
            class Query extends AbstractQuery {}

            namespace App;
            use Doctrine\ORM\Query;
            class Repo1 {
                public function __construct(private Query $q) {}
                /** @return list<int> */
                public function test(): array {
                    return $this->q->getResult();
                }
            }
        