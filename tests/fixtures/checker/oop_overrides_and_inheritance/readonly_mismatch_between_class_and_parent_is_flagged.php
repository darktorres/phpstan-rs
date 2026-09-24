<?php
            class Mutable1 {}
            readonly class Frozen1 {}
            readonly class FrozenSub extends Mutable1 {}
            class MutableSub extends Frozen1 {}
            readonly class FrozenOk extends Frozen1 {}
        