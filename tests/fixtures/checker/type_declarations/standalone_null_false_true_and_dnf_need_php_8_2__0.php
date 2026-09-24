<?php
        interface A {} interface B {}
        function a(null $x, ?false $y, null|false $z, int|false $w): true { return true; }
        function b((A&B)|null $x): void {}
        