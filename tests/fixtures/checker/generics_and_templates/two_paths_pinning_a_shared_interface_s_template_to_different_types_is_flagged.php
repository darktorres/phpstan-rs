<?php
            /** @template Y */
            interface BaseIface {}
            /**
             * @template X
             * @extends BaseIface<X>
             */
            interface AIface extends BaseIface {}
            /**
             * @template Z
             * @extends BaseIface<Z>
             */
            interface BIface extends BaseIface {}
            /**
             * @template T
             * @implements AIface<T>
             * @implements BIface<int>
             */
            class Box implements AIface, BIface {}
        