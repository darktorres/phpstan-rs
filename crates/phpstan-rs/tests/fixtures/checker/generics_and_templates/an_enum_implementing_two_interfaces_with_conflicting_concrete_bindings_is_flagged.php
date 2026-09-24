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
             * @implements AIface<int>
             * @implements BIface<string>
             */
            enum Suit implements AIface, BIface {}
        