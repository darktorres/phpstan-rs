<?php
            use function PHPStan\Testing\assertType;
            interface Transport15 {}
            interface CountAware15 {}
            /** @template-covariant T of Transport15 */
            interface FactoryInterface15 {
                /** @return T */
                public function create(): Transport15;
            }
            final class RealTransport15 implements Transport15, CountAware15 {}
            /** @implements FactoryInterface15<RealTransport15> */
            final class RealFactory15 implements FactoryInterface15 {
                public function create(): Transport15 {
                    return new RealTransport15();
                }
            }
            function f(): Transport15&CountAware15 {
                return (new RealFactory15())->create();
            }
            function g(): void {
                assertType('RealTransport15', (new RealFactory15())->create());
            }
        