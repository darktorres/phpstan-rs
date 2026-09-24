<?php
            use function PHPStan\Testing\assertType;
            class Base {
                public ?string $flag = null;
                /** @phpstan-assert-if-true !null $this->flag */
                public function isReady(): bool { return $this->flag !== null; }
            }
            class A extends Base {}
            class B extends Base {}
            function run(A|B $v): void {
                if ($v->isReady()) {
                    assertType('string', $v->flag);
                }
            }
        