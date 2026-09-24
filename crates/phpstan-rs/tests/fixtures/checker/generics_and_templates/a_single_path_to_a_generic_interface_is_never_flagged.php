<?php
            /** @template Y */
            interface BaseIface {}
            /**
             * @template X
             * @extends BaseIface<X>
             */
            interface AIface extends BaseIface {}
            /** @template T
             *   @implements AIface<T>
             */
            class Box implements AIface {}
        