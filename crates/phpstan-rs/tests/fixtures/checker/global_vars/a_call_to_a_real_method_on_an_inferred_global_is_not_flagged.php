<?php
            class Template {
                function assign($name, $value) {}
            }
            $template = new Template();
            function render() {
                global $template;
                $template->assign('x', 1);
            }
        