<?php
            function setValue33(int &$x): void { $x = 1; }
            function test(array $arr): void { setValue33($arr[0]); }
        