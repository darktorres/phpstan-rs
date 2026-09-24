<?php
        /** @return true */
        function alwaysTrue(): bool { return true; }
        function f(): void { if (alwaysTrue()) { echo 1; } }
        