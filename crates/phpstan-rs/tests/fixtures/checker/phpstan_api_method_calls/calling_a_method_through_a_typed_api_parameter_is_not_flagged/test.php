<?php
        namespace PHPStan\FakeNs;
        /** @api */
        class Widget {
            public function getType() {}
        }
        namespace App;
        function f(\PHPStan\FakeNs\Widget $scope) {
            $scope->getType();
        }
        