<?php
            use function PHPStan\Testing\assertType;
            class Base {
                public function fluent(): static { return $this; }
                /** @return $this */
                public function me(): static { return $this; }
                public function selfy(): self { return $this; }
                /** @var self|null */
                public $link = null;
            }
            class Sub extends Base {}
            function f(Sub $s): void {
                assertType('Sub', $s->fluent());
                assertType('Sub', $s->me());
                assertType('Base', $s->selfy());
                assertType('Base|null', $s->link);
            }
