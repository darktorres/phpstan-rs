<?php
            interface Doer { public function doIt(): void; }
            function test(): Doer { return new Doer(); }
        