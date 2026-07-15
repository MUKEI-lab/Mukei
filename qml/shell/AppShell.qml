import QtQuick

Item {
    id: root
    clip: true

    AdaptiveNavigation {
        id: adaptiveNavigation
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        z: 20
    }

    RouterHost {
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.right: parent.right
        anchors.left: adaptiveNavigation.visible ? adaptiveNavigation.right : parent.left
    }

    BannerHost {
        anchors.left: adaptiveNavigation.visible ? adaptiveNavigation.right : parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        z: 50
    }

    SnackbarHost {
        anchors.left: adaptiveNavigation.visible ? adaptiveNavigation.right : parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        z: 80
    }

    OperationOverlayHost {
        anchors.top: parent.top
        anchors.right: parent.right
        z: 60
    }

    SheetHost { anchors.fill: parent; z: 70 }
    DialogHost { anchors.fill: parent; z: 90 }
}
