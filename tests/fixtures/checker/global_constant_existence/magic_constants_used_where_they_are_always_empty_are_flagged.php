<?php
            echo __CLASS__, __TRAIT__, __FUNCTION__, __METHOD__, __NAMESPACE__;
            $c = function () { return __FUNCTION__; };
            function f(): string { return __FUNCTION__ . __CLASS__; }
            trait T { public function m(): string { return __TRAIT__ . __CLASS__; } }
            class K { public function m(): string { return __CLASS__ . __METHOD__ . __TRAIT__; } }
        