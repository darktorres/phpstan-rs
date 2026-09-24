<?php
            /** @template T */
            class Base {}
            /** @template T */
            interface Coll {}
            /** @template-covariant T */
            interface Out {}
            /** @template T */
            trait Tr {}
            class Plain {}
            /** @extends Base<int> */
            class A extends Plain {}
            /** @extends Base<int> */
            class B {}
            class C extends Base {}
            /**
             * @template-covariant U
             * @extends Base<U>
             * @implements Coll<Missing>
             */
            class D extends Base implements Coll {}
            /** @implements Out<covariant int> */
            class E implements Out {}
            /** @implements Coll<int> */
            interface F extends Coll {}
            /** @extends Coll<int> */
            enum G implements Coll { case X; }
            class H {
                /** @use Tr<int> */
                use Tr;
            }
            class I { use Tr; }
        