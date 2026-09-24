<?php
            namespace App;
            use Other\Thing;
            class Foo {}
            function a(int|INT $x, Foo|\App\Foo $y, Thing|\Other\Thing $z): void {}
            function b(bool|false $x, true|false $y, object|Foo $z, iterable|array $w): void {}
            function c(): object|static {}
            function d(): object|self {}
            