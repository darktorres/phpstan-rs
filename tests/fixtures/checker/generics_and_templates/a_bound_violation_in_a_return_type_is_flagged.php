<?php
            interface Shape {}
            class NotAShape {}
            /**
             * @template T of Shape
             */
            class Collection {}
            /** @return Collection<NotAShape> */
            function f() {}
        