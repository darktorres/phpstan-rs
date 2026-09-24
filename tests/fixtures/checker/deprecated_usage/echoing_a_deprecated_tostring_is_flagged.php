<?php
            class OldThing {
                /** @deprecated do not stringify */
                public function __toString(): string { return "y"; }
            }
            function f(OldThing $o): void {
                echo $o;
            }
        