<?php
            interface Shape {}
            class Circle implements Shape {}
            /** @param Shape|null $s */
            function f(Circle $s): void {}
        