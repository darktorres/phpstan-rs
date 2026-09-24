<?php
            /** @deprecated */
            function oldFunc(): void {}
            function caller(): void {
                oldFunc();
            }
        