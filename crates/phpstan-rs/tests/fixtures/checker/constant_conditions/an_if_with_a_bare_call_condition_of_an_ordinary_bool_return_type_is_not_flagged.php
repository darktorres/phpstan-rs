<?php
        function maybe(): bool { return random_int(0, 1) === 1; }
        function f(): void { if (maybe()) { echo 1; } }
        