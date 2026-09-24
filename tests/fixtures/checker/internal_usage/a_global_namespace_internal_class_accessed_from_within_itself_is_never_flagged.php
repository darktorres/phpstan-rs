<?php
            /** @internal */
            class Thing {
                public function doStuff(): void {}
                public function other(): void {
                    $this->doStuff();
                }
            }
        