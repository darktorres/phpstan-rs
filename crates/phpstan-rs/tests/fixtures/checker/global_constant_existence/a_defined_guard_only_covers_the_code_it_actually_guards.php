<?php
            defined('VIA_OR') || define('VIA_OR', 1);
            function inside(): void {
                if (defined('ONLY_IN_BRANCH')) { echo ONLY_IN_BRANCH; }
                echo ONLY_IN_BRANCH;
            }
            function early(): void {
                if (!defined('AFTER_RETURN')) { return; }
                echo AFTER_RETURN;
            }
            function viaOr(): void { echo VIA_OR; }
        