<?php
                class Template {
                    function assign($name, $value) {}
                }
                function f(Template $t) {
                    $t->assign('x', 1);
                }
            