<?php
            class Open { public static function make(): static { return new static(); } }
            final class Closed { public static function make(): static { return new static(); } }
            /** @final */
            class SoftClosed { public static function make(): static { return new static(); } }
            /** @phpstan-consistent-constructor */
            class Consistent { public static function make(): static { return new static(); } }
            class ConsistentChild extends Consistent {
                public function m(): static { return new static(); }
            }
            class FinalCtor {
                final public function __construct() {}
                public function m(): static { return new static(); }
            }
            interface HasCtor { public function __construct(); }
            class ViaIface implements HasCtor {
                public function __construct() {}
                public function m(): static { return new static(); }
            }
            abstract class AbsProto { abstract public function __construct(); }
            class ProtoImpl extends AbsProto {
                public function __construct() {}
                public function m(): static { return new static(); }
            }
            trait NeedsCtor {
                abstract public function __construct();
                public function m(): static { return new static(); }
            }
            class UsesNeedsCtor {
                use NeedsCtor;
                public function __construct() {}
            }
        