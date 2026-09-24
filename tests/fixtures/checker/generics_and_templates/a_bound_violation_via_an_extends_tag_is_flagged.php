<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @extends Collection<NotAShape> */
            class Bad extends Collection {}
        